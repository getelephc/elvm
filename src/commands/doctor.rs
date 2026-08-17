use crate::installed::{self, Installed, BRIDGE_ARCHIVES};
use crate::paths::ElvmPaths;
use crate::target;
use std::path::PathBuf;

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
    if shim.exists() {
        println!("✓ shim present at {}", shim.display());
    } else {
        problems += 1;
        println!("✗ shim missing at {}", shim.display());
        println!(
            "  reinstall elvm, or recreate it: ln -s elvm {}",
            shim.display()
        );
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
            println!("  reinstall with: elvm install {name} --force");
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
