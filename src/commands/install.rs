use crate::github;
use crate::install;
use crate::paths::ElvmPaths;
use crate::version::VersionRequest;

pub fn run(paths: &ElvmPaths, version: Option<&str>, force: bool) -> anyhow::Result<()> {
    // There is no early, offline check for "does elephc publish a binary
    // for this host" here any more: whether one exists is a property of
    // the release data (which release, which host — see
    // `install::no_binary_error`), not a fixed platform list that could be
    // consulted before the network call that fetches the release list. A
    // rate limit or dropped connection during that fetch now surfaces as
    // exactly that — a network error — which is honest, since at that point
    // elvm genuinely does not yet know whether a binary exists.
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
    // Routed before the release list is fetched: a nightly is addressed by
    // tag through its own endpoint, and never appears among releases.
    if let VersionRequest::Nightly(channel) = &request {
        return install::nightly(paths, channel, force);
    }

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
