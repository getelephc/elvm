use crate::archive;
use crate::download;
use crate::github;
use crate::installed;
use crate::lock::InstallLock;
use crate::paths::ElvmPaths;
use crate::target;
use semver::Version;
use std::path::Path;

/// Downloads, verifies, and atomically installs a published release.
pub fn from_release(paths: &ElvmPaths, version: &Version, force: bool) -> anyhow::Result<()> {
    let host = target::host()?;
    if !target::elephc_publishes(&host) {
        anyhow::bail!(
            "no published binary for {host}\n  \
             elephc publishes macOS ARM64 only; build it instead:\n  \
             elvm install --build v{version}"
        );
    }

    paths.ensure_dirs()?;
    let _lock = InstallLock::acquire(paths)?;

    let destination = paths.version_dir(&version.to_string());
    if destination.exists() && !force {
        anyhow::bail!(
            "elephc {version} is already installed\n  reinstall with: elvm install {version} --force"
        );
    }

    let releases = github::list_releases(paths, false)?;
    let release = releases
        .iter()
        .find(|r| r.version == *version)
        .ok_or_else(|| anyhow::anyhow!("elephc {version} is not a published release"))?;

    let tarball_name = github::tarball_name(version, &host);
    let checksum_name = github::checksum_name(version, &host);

    let tarball_asset = release
        .asset(&tarball_name)
        .ok_or_else(|| anyhow::anyhow!("release v{version} has no asset {tarball_name}"))?;
    let checksum_asset = release
        .asset(&checksum_name)
        .ok_or_else(|| anyhow::anyhow!("release v{version} has no asset {checksum_name}"))?;

    let expected = download::parse_checksum_file(&download::fetch_text(&checksum_asset.url)?)?;
    let cached = paths.downloads().join(&tarball_name);

    println!("downloading elephc {version}");
    download::fetch_verified(&tarball_asset.url, &expected, &cached)?;

    let staging = tempfile::Builder::new()
        .prefix("install")
        .tempdir_in(paths.tmp())?;
    archive::extract_tar_gz(&cached, staging.path())?;

    install_staged(
        &destination,
        staging,
        "the downloaded archive is missing the elephc binary or its bridge archives",
    )?;
    println!("installed elephc {version}");
    Ok(())
}

/// Completes an installation by verifying completeness and atomically
/// replacing the destination with the staging directory.
///
/// This defers all destructive operations until after the completeness check,
/// keeping a working prior installation in place until the last moment.
pub fn install_staged(
    destination: &Path,
    staging: tempfile::TempDir,
    error_msg: &str,
) -> anyhow::Result<()> {
    if !installed::is_complete(staging.path()) {
        anyhow::bail!("{error_msg}");
    }

    // Everything that can fail — the release lookup, checksum fetch,
    // download, extraction, and completeness check — has already happened
    // above, against the staging directory. Only now, right before the
    // rename that replaces it, do we touch the existing installation: this
    // shrinks the destructive window from the whole network-and-extraction
    // phase down to these two adjacent syscalls. The rename itself is
    // atomic (staging and versions/ share a filesystem), so a crash here
    // leaves a stale tmp directory, never a half-installed version — but a
    // crash between the remove and the rename can still leave `force`
    // reinstalls with nothing in place, which is the best this two-step
    // swap (no atomic directory replace exists in POSIX) can offer.
    let staged = staging.keep();
    if destination.exists() {
        std::fs::remove_dir_all(destination)?;
    }
    std::fs::rename(staged, destination)?;
    Ok(())
}
