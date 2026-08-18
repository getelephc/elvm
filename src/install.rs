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

    // Whether a binary for `host` exists is a property of the release data,
    // not a fixed set of platforms — so this is discovered by asking the
    // release itself, same as `ls_remote::has_binary`, rather than
    // consulting a constant. When it's missing, `no_binary_error` searches
    // every release for the earliest one that does carry it, which is
    // almost always more useful than "build it yourself".
    let tarball_asset = release
        .asset(&tarball_name)
        .ok_or_else(|| no_binary_error(&releases, version, &host))?;
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

/// Builds the error for a requested version with no binary for `host`.
///
/// Names whichever published release is the *earliest* to carry one for
/// this host, found by scanning every release rather than a fixed platform
/// list — that's almost always the actionable fix (install a version that
/// works, or build this exact one). `--build` is only suggested when no
/// release at all has a binary for this host, since only then is it true
/// that nothing short of a local build will do.
fn no_binary_error(releases: &[github::Release], version: &Version, host: &str) -> anyhow::Error {
    let earliest = releases
        .iter()
        .filter(|r| r.asset(&github::tarball_name(&r.version, host)).is_some())
        .map(|r| &r.version)
        .min();

    match earliest {
        Some(earliest) => anyhow::anyhow!(
            "no published binary for {host} in elephc {version}\n  \
             the earliest release with one is {earliest}; install that instead:\n  \
             elvm install {earliest}"
        ),
        None => anyhow::anyhow!(
            "no published binary for {host}\n  \
             no elephc release publishes one for this platform; build it instead:\n  \
             elvm install --build v{version}"
        ),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{Asset, Release};

    fn release(version: &str, target: Option<&str>) -> Release {
        let version = Version::parse(version).unwrap();
        let assets = match target {
            Some(target) => vec![Asset {
                name: github::tarball_name(&version, target),
                url: String::new(),
            }],
            None => Vec::new(),
        };
        Release { version, assets }
    }

    #[test]
    fn no_binary_error_names_the_earliest_release_that_has_one() {
        let releases = vec![
            release("0.26.4", Some("x86_64-unknown-linux-gnu")),
            release("0.25.2", Some("x86_64-unknown-linux-gnu")),
            release("0.24.3", Some("aarch64-apple-darwin")),
        ];
        let err = no_binary_error(
            &releases,
            &Version::parse("0.24.3").unwrap(),
            "x86_64-unknown-linux-gnu",
        );
        let message = err.to_string();
        assert!(message.contains("0.24.3"), "{message}");
        assert!(message.contains("0.25.2"), "{message}");
        assert!(message.contains("elvm install 0.25.2"), "{message}");
        // A version this platform *can* install exists, so the fix must not
        // fall back to suggesting a from-source build.
        assert!(!message.contains("--build"), "{message}");
    }

    #[test]
    fn no_binary_error_falls_back_to_build_when_no_release_has_one() {
        let releases = vec![release("0.26.4", Some("aarch64-apple-darwin"))];
        let err = no_binary_error(
            &releases,
            &Version::parse("0.26.4").unwrap(),
            "riscv64gc-unknown-linux-gnu",
        );
        let message = err.to_string();
        assert!(message.contains("--build"), "{message}");
        assert!(
            message.contains("elvm install --build v0.26.4"),
            "{message}"
        );
    }
}
