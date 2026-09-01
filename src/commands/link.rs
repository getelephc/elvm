use crate::installed;
use crate::paths::ElvmPaths;
use std::path::{Path, PathBuf};

/// Registers a local checkout under `versions/<name>` as a symlink.
///
/// A checkout's `target/release` already holds the bridge archives beside the
/// binary, so elephc's discovery works there with no extra work.
pub fn run(paths: &ElvmPaths, raw_path: &str, name: &str) -> anyhow::Result<()> {
    if semver::Version::parse(name).is_ok() {
        anyhow::bail!("{name} looks like a release version; pick a name like \"dev\"");
    }
    // `versions/nightly` and `versions/nightly-<date>` are where the nightly
    // channel installs. Letting a link take one of those names would make
    // `elvm install nightly` overwrite a checkout the user still points at.
    if crate::nightly::Channel::parse(name).is_some() {
        anyhow::bail!("{name} is reserved for the nightly channel; pick a name like \"dev\"");
    }

    let given = if std::path::Path::new(raw_path).is_absolute() {
        PathBuf::from(raw_path)
    } else {
        std::env::current_dir()?.join(raw_path)
    };
    if !given.exists() {
        anyhow::bail!("cannot read {raw_path}: No such file or directory");
    }
    let target = resolve_binary_dir(&given)?;

    if !installed::is_complete(&target) {
        eprintln!(
            "warning: {} has no bridge archives; builds needing --with-pdo or --with-tls will fail",
            target.display()
        );
    }

    paths.ensure_dirs()?;
    let link = paths.version_dir(name);
    if link.exists() || std::fs::symlink_metadata(&link).is_ok() {
        anyhow::bail!("{name} already exists; remove it with: elvm uninstall {name}");
    }
    std::os::unix::fs::symlink(&target, &link)?;
    println!("linked {} as {name}", target.display());
    Ok(())
}

/// Accepts either a directory holding `elephc` or a repository root whose
/// `target/release` does.
fn resolve_binary_dir(given: &Path) -> anyhow::Result<PathBuf> {
    if given.join("elephc").is_file() {
        return Ok(given.to_path_buf());
    }
    let built = given.join("target/release");
    if built.join("elephc").is_file() {
        return Ok(built);
    }
    anyhow::bail!(
        "no elephc binary in {} or {}\n  build it first: cargo build --release",
        given.display(),
        built.display()
    )
}
