use crate::github;
use crate::install;
use crate::paths::ElvmPaths;
use crate::target;
use crate::version::VersionRequest;

pub fn run(paths: &ElvmPaths, version: Option<&str>, force: bool) -> anyhow::Result<()> {
    // Checked before any network call: on CI runners and other non-macOS
    // hosts this is exactly where a request would otherwise die of a rate
    // limit or a dropped connection first, masking the real reason
    // (elephc simply isn't published here) behind an unrelated network
    // error. `install::from_release` checks this again for callers that
    // reach it directly, but the user-facing path must fail fast.
    let host = target::host()?;
    if !target::elephc_publishes(&host) {
        anyhow::bail!(
            "no published binary for {host}\n  \
             elephc publishes macOS ARM64 only; build it instead:\n  \
             elvm install --build"
        );
    }

    let raw = match version {
        Some(value) => value.to_string(),
        None => {
            let request = super::active_request(paths)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "no version given and no .elephc-version found\n  \
                     try: elvm install latest"
                )
            })?;
            request.raw
        }
    };

    let request = VersionRequest::parse(&raw);
    let published: Vec<semver::Version> = github::list_releases(paths, false)?
        .into_iter()
        .map(|release| release.version)
        .collect();

    let version = match &request {
        // `latest` means "newest published" here, unlike in resolution, where
        // it means "highest installed" — the spec's deliberate asymmetry.
        VersionRequest::Latest => published
            .iter()
            .max()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no published releases found"))?,
        other => other.select(&published).ok_or_else(|| {
            anyhow::anyhow!("no published elephc release matches {raw}\n  see: elvm ls-remote")
        })?,
    };

    install::from_release(paths, &version, force)
}
