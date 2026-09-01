use crate::archive;
use crate::download;
use crate::github;
use crate::installed;
use crate::lock::InstallLock;
use crate::nightly::{Channel, Stamp};
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

/// Downloads, verifies, and installs a nightly build.
///
/// Three things separate this from `from_release`:
///
///   * reinstalling is not an error. `nightly` is a channel, not a version,
///     so `elvm install nightly` on an already-installed nightly is an
///     update — the same shape as `rustup update nightly`.
///   * what is installed is identified by the tarball's SHA-256, not by its
///     version string. Two builds from the same UTC day differ only in the
///     `+g<sha>` build metadata, which semver comparisons ignore; the digest
///     is fetched anyway to verify the download, so it costs nothing.
///   * what was installed is always printed. The workflow skips nights when
///     `main` is red or unchanged, so the newest nightly can be days old and
///     a silent "installed" would hide that.
pub fn nightly(paths: &ElvmPaths, channel: &Channel, force: bool) -> anyhow::Result<()> {
    let host = target::host()?;

    paths.ensure_dirs()?;
    let _lock = InstallLock::acquire(paths)?;

    let release = github::fetch_nightly(channel.tag())?
        .ok_or_else(|| unpublished_nightly_error(paths, channel))?;

    let tarball_name = github::nightly_tarball_name(&host);
    let checksum_name = github::nightly_checksum_name(&host);
    let tarball_asset = release
        .asset(&tarball_name)
        .ok_or_else(|| no_nightly_binary_error(&release, &host))?;
    let checksum_asset = release
        .asset(&checksum_name)
        .ok_or_else(|| anyhow::anyhow!("nightly {} has no asset {checksum_name}", release.tag))?;

    let expected = download::parse_checksum_file(&download::fetch_text(&checksum_asset.url)?)?;
    let stamp = Stamp {
        tag: release.tag.clone(),
        version: release.version.clone(),
        commit: release.commit.clone(),
        published_at: release.published_at.clone(),
        sha256: expected.clone(),
    };

    let destination = paths.version_dir(channel.dir_name());
    if !force && installed::is_complete(&destination) {
        if let Some(current) = Stamp::read(&destination) {
            if current.sha256.eq_ignore_ascii_case(&expected) {
                if channel.is_rolling() {
                    println!("elephc nightly is already up to date: {}", stamp.summary());
                } else {
                    println!("elephc {channel} is already installed: {}", stamp.summary());
                    println!("  reinstall it with: elvm install {channel} --force");
                }
                return Ok(());
            }
        }
    }

    // Named by version rather than by the upstream filename, which carries no
    // version at all: every nightly ships as `elephc-nightly-<triple>.tar.gz`,
    // so caching under that name would have each build overwrite the last.
    // Keyed this way, `cache/downloads/` keeps nightlies that upstream has
    // already pruned — the only copy of a build older than the retention
    // window that elvm can reinstall from.
    let cached = paths
        .downloads()
        .join(format!("elephc-{}-{host}.tar.gz", release.version));

    println!("downloading elephc {} ({})", release.version, release.tag);
    download::fetch_verified(&tarball_asset.url, &expected, &cached)?;

    let staging = tempfile::Builder::new()
        .prefix("install")
        .tempdir_in(paths.tmp())?;
    archive::extract_tar_gz(&cached, staging.path())?;
    // Written before the swap, so the stamp lands atomically with the files
    // it describes: a directory can never claim to hold a build it does not.
    stamp.write(staging.path())?;

    install_staged(
        &destination,
        staging,
        "the downloaded nightly archive is missing the elephc binary or its bridge archives",
    )?;

    println!("installed elephc {} as {channel}", stamp.summary());
    if channel.is_rolling() {
        println!("  this is a moving target; pin a dated build with: elvm ls-remote");
    }
    Ok(())
}

/// Builds the error for a nightly tag that is not published.
///
/// For the rolling channel that means the workflow has not published one yet.
/// For a dated tag it usually means the retention window has passed it by, so
/// the message names the oldest one still available — the listing costs a
/// request, which is why it happens only on this path.
fn unpublished_nightly_error(paths: &ElvmPaths, channel: &Channel) -> anyhow::Error {
    if channel.is_rolling() {
        return anyhow::anyhow!(
            "the nightly channel has no published build\n  \
             nightlies are built from `main` and skipped when it is red or unchanged\n  \
             see what is published: elvm ls-remote"
        );
    }

    let available = github::list_nightlies(paths, true).unwrap_or_default();
    match (available.first(), available.last()) {
        (Some(newest), Some(oldest)) => anyhow::anyhow!(
            "elephc {channel} is not published\n  \
             dated nightlies are kept for a limited window and then deleted upstream\n  \
             the oldest still published is {}, the newest is {}\n  \
             see them all: elvm ls-remote",
            oldest.tag,
            newest.tag
        ),
        _ => anyhow::anyhow!(
            "elephc {channel} is not published, and no dated nightly is\n  \
             install the rolling channel instead: elvm install nightly"
        ),
    }
}

/// Builds the error for a nightly with no binary for `host`.
///
/// Unlike a release, a nightly cannot be answered with "install an older one
/// that has a binary": every nightly publishes the same three targets, so if
/// this one has no binary for the host, none of them will. The actionable
/// fix is a build from source at that exact commit.
fn no_nightly_binary_error(release: &github::NightlyRelease, host: &str) -> anyhow::Error {
    let targets = release.targets();
    let published = if targets.is_empty() {
        "none".to_string()
    } else {
        targets.join(", ")
    };
    anyhow::anyhow!(
        "no nightly binary for {host}\n  \
         nightly {} publishes: {published}\n  \
         build that commit instead: elvm install --build {}",
        release.tag,
        release.commit
    )
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
