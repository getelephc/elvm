use crate::installed::BRIDGE_ARCHIVES;
use crate::lock::InstallLock;
use crate::paths::ElvmPaths;
use std::path::Path;
use std::process::Command;

const REPO_URL: &str = "https://github.com/illegalstudio/elephc.git";

/// The bridge packages elephc's own release workflow builds after the binary.
const BRIDGE_PACKAGES: [&str; 8] = [
    "elephc-tls",
    "elephc-pdo",
    "elephc-crypto",
    "elephc-bcmath",
    "elephc-phar",
    "elephc-tz",
    "elephc-image",
    "elephc-web",
];

/// Builds elephc from source at `git_ref` and installs it like a release.
///
/// This mirrors elephc's release workflow exactly — binary first, then the
/// eight bridge staticlibs — so a local build is indistinguishable from a
/// downloaded one.
pub fn from_source(paths: &ElvmPaths, git_ref: &str, force: bool) -> anyhow::Result<()> {
    require_tool("cargo")?;
    require_tool("git")?;

    paths.ensure_dirs()?;
    let _lock = InstallLock::acquire(paths)?;

    let name = git_ref.trim_start_matches('v').to_string();
    let destination = paths.version_dir(&name);
    if destination.exists() && !force {
        anyhow::bail!(
            "elephc {name} is already installed\n  rebuild with: elvm install --build {git_ref} --force"
        );
    }

    let mirror = paths.cache().join("elephc.git");
    update_mirror(&mirror)?;

    let worktree = tempfile::Builder::new()
        .prefix("build")
        .tempdir_in(paths.tmp())?;
    run(Command::new("git").args([
        "clone",
        "--shared",
        mirror.to_str().unwrap(),
        worktree.path().to_str().unwrap(),
    ]))?;
    run(Command::new("git")
        .current_dir(worktree.path())
        .args(["checkout", git_ref]))?;

    println!("building elephc {name} (this takes a while)");
    run(Command::new("cargo")
        .current_dir(worktree.path())
        .args(["build", "--release"]))?;

    let mut bridges = vec!["build".to_string(), "--release".to_string()];
    for package in BRIDGE_PACKAGES {
        bridges.push("-p".to_string());
        bridges.push(package.to_string());
    }
    run(Command::new("cargo")
        .current_dir(worktree.path())
        .args(&bridges))?;

    let built = worktree.path().join("target/release");
    let _ = run(Command::new("strip").arg(built.join("elephc")));

    let staging = tempfile::Builder::new()
        .prefix("stage")
        .tempdir_in(paths.tmp())?;
    std::fs::copy(built.join("elephc"), staging.path().join("elephc"))?;
    for archive in BRIDGE_ARCHIVES {
        let from = built.join(archive);
        if from.is_file() {
            std::fs::copy(from, staging.path().join(archive))?;
        }
    }

    crate::install::install_staged(
        &destination,
        staging,
        &name,
        "the build did not produce every bridge archive; see the cargo output above",
    )?;
    println!("installed elephc {name} from source");
    Ok(())
}

fn require_tool(tool: &str) -> anyhow::Result<()> {
    which(tool).ok_or_else(|| {
        anyhow::anyhow!("{tool} is required for --build but was not found on PATH")
    })?;
    Ok(())
}

fn which(tool: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(tool))
        .find(|candidate| candidate.is_file())
}

fn update_mirror(mirror: &Path) -> anyhow::Result<()> {
    if mirror.exists() {
        run(Command::new("git")
            .current_dir(mirror)
            .args(["fetch", "--all", "--tags", "--prune"]))
    } else {
        if let Some(parent) = mirror.parent() {
            std::fs::create_dir_all(parent)?;
        }
        run(Command::new("git").args(["clone", "--bare", REPO_URL, mirror.to_str().unwrap()]))
    }
}

fn run(command: &mut Command) -> anyhow::Result<()> {
    let status = command
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run {:?}: {e}", command.get_program()))?;
    if !status.success() {
        anyhow::bail!("{:?} failed with {status}", command.get_program());
    }
    Ok(())
}
