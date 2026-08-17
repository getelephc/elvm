use crate::archive;
use crate::download;
use crate::github;
use crate::installed;
use crate::lock::InstallLock;
use crate::paths::ElvmPaths;
use crate::target;
use semver::Version;

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
    if destination.exists() {
        if !force {
            anyhow::bail!(
                "elephc {version} is already installed\n  reinstall with: elvm install {version} --force"
            );
        }
        std::fs::remove_dir_all(&destination)?;
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

    if !installed::is_complete(staging.path()) {
        anyhow::bail!("the downloaded archive is missing the elephc binary or its bridge archives");
    }

    // Atomic: staging and versions/ share a filesystem, so a crash leaves a
    // stale tmp directory rather than a half-installed version.
    std::fs::rename(staging.keep(), &destination)?;
    println!("installed elephc {version}");
    Ok(())
}
