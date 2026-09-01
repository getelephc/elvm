use crate::installed::{self, Installed, BRIDGE_ARCHIVES};
use crate::nightly::{self, Channel, Stamp};
use crate::paths::ElvmPaths;
use crate::target;
use std::path::PathBuf;

/// How many nightlies elephc keeps published. Used only to tell someone
/// their installed nightly has aged out of that window, so being wrong here
/// costs a slightly early or late note, never a wrong action.
const RETENTION_DAYS: i64 = 14;

/// Reports environment problems, each with the command that fixes it.
pub fn run(paths: &ElvmPaths) -> anyhow::Result<()> {
    let mut problems = 0;

    let bin = paths.bin();
    let entries: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();

    match entries.iter().position(|dir| *dir == bin) {
        None => {
            problems += 1;
            println!("✗ {} is not on PATH", bin.display());
            println!("  add it with: eval \"$(elvm init zsh)\"  (or see: elvm init sh)");
        }
        Some(index) => {
            println!("✓ {} is on PATH", bin.display());
            // A Homebrew elephc earlier on PATH silently wins over the shim,
            // which is the likeliest source of confusing behaviour.
            if let Some(shadow) = entries
                .iter()
                .take(index)
                .map(|dir| dir.join("elephc"))
                .find(|candidate| candidate.is_file())
            {
                problems += 1;
                println!(
                    "✗ {} comes before the shim and will shadow it",
                    shadow.display()
                );
                println!("  move {} earlier in PATH", bin.display());
            }
        }
    }

    let shim = bin.join("elephc");
    // `shim.exists()` alone follows the symlink and only proves *something*
    // is there: a shim retargeted at a stale or unrelated file that still
    // happens to exist would report ✓ even though it is not the relative
    // `elvm` link the installer creates (§4.3). Reading the link itself
    // catches that; a missing or non-symlink shim still reports the same
    // "missing" fix.
    match std::fs::read_link(&shim) {
        Ok(target) if target == std::path::Path::new("elvm") => {
            println!("✓ shim present at {}", shim.display());
        }
        Ok(target) => {
            problems += 1;
            println!(
                "✗ shim at {} points at {} instead of elvm",
                shim.display(),
                target.display()
            );
            println!("  recreate it with: ln -sf elvm {}", shim.display());
        }
        Err(_) => {
            problems += 1;
            println!("✗ shim missing at {}", shim.display());
            println!(
                "  reinstall elvm, or recreate it: ln -sf elvm {}",
                shim.display()
            );
        }
    }

    let root = paths.root();
    if root.exists() {
        if let Err(err) = check_writable(root) {
            problems += 1;
            println!("✗ {} is not writable: {err}", root.display());
            println!("  fix its permissions: chmod u+w {}", root.display());
        } else {
            println!("✓ {} is writable", root.display());
        }
    }

    println!("✓ host target: {}", target::host()?);

    let installed = Installed::scan(paths)?;
    if installed.versions.is_empty() && installed.aliases.is_empty() {
        println!("· no versions installed — try: elvm install latest");
    }
    for name in installed
        .versions
        .iter()
        .map(|v| v.to_string())
        .chain(installed.aliases.iter().cloned())
    {
        let dir = paths.version_dir(&name);
        if installed::is_complete(&dir) {
            println!("✓ {name} complete");
        } else {
            problems += 1;
            let missing: Vec<&str> = BRIDGE_ARCHIVES
                .iter()
                .copied()
                .filter(|a| !dir.join(a).is_file())
                .collect();
            println!(
                "✗ {name} is missing bridge archive(s): {}",
                missing.join(", ")
            );
            // `elvm install <name> --force` only makes sense for a name
            // elvm can fetch: a semver version, or a nightly tag. An alias
            // created by `elvm link` has no published artifact to reinstall
            // from, so `elvm install dev --force` fails with "no published
            // elephc release matches dev".
            if semver::Version::parse(&name).is_ok() || Channel::parse(&name).is_some() {
                println!("  reinstall with: elvm install {name} --force");
            } else {
                println!(
                    "  re-link it: elvm link <path> --as {name}  (or remove it: elvm uninstall {name})"
                );
            }
        }
        if let Some(stamp) = Stamp::read(&dir) {
            println!("  {}", stamp.summary());
            // Not a problem, so it does not fail `doctor`: an aged-out
            // nightly still works, it just can no longer be downloaded, so
            // a colleague given this version string cannot install it.
            if let Some(age) = stamp.age_in_days(nightly::now_unix()) {
                if age > RETENTION_DAYS {
                    println!(
                        "  · {age} days old; upstream keeps {RETENTION_DAYS} nightlies, so this build is no longer published"
                    );
                    println!("    update it with: elvm install nightly");
                }
            }
        }
        if let Some(reason) = quarantined(&dir.join("elephc")) {
            problems += 1;
            println!("✗ {name}: {reason}");
            println!("  clear it with: xattr -cr {}", dir.display());
        }
    }

    if problems > 0 {
        anyhow::bail!("{problems} problem(s) found");
    }
    println!("\nno problems found");
    Ok(())
}

/// A real round-trip rather than a permission-bit check: bits alone don't
/// account for ownership, ACLs, or a read-only filesystem.
fn check_writable(root: &std::path::Path) -> std::io::Result<()> {
    let probe = root.join(".elvm-doctor-writable");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(&probe)
}

/// macOS only, and normally never true: elvm downloads with its own HTTP
/// client, which does not set the quarantine attribute. It can only appear if
/// a browser-downloaded archive was placed into the cache by hand.
fn quarantined(binary: &std::path::Path) -> Option<String> {
    if !cfg!(target_os = "macos") || !binary.is_file() {
        return None;
    }
    let output = std::process::Command::new("xattr")
        .arg("-p")
        .arg("com.apple.quarantine")
        .arg(binary)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| "binary carries com.apple.quarantine and Gatekeeper will block it".to_string())
}
