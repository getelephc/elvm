use crate::nightly::Channel;
use crate::paths::ElvmPaths;
use semver::Version;
use serde::Deserialize;

/// A published elephc release, reduced to what elvm needs.
#[derive(Debug, Clone)]
pub struct Release {
    pub version: Version,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

const DEFAULT_API: &str = "https://api.github.com";
const REPO: &str = "illegalstudio/elephc";
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(600);

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    /// `Nightly <version>` on a nightly release. Unused for releases, whose
    /// version comes from the tag.
    #[serde(default)]
    name: String,
    #[serde(default)]
    target_commitish: String,
    /// When the release was published. Deliberately not `created_at`, which
    /// is the *tag's* timestamp and runs ahead of publication by however long
    /// the build took — 49 minutes on the first nightly.
    #[serde(default)]
    published_at: String,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// A published nightly, from either the rolling tag or a dated one.
///
/// `version` is kept as a string on purpose: it parses as semver, but
/// comparing nightlies that way is wrong twice over (see `crate::nightly`),
/// so elvm never puts one in a `Version`.
#[derive(Debug, Clone)]
pub struct NightlyRelease {
    pub tag: String,
    pub version: String,
    pub commit: String,
    pub published_at: String,
    pub assets: Vec<Asset>,
}

impl NightlyRelease {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// The triples this nightly actually publishes, for the error shown when
    /// the host is not among them.
    pub fn targets(&self) -> Vec<String> {
        self.assets
            .iter()
            .filter_map(|a| {
                a.name
                    .strip_prefix("elephc-nightly-")?
                    .strip_suffix(".tar.gz")
                    .map(str::to_string)
            })
            .collect()
    }
}

pub fn tarball_name(version: &Version, target: &str) -> String {
    format!("elephc-v{version}-{target}.tar.gz")
}

pub fn checksum_name(version: &Version, target: &str) -> String {
    format!("{}.sha256", tarball_name(version, target))
}

/// Nightly assets carry no version in their name — the tag disambiguates the
/// URL — so the same filename appears under `nightly` and under every dated
/// tag, holding the same bytes.
pub fn nightly_tarball_name(target: &str) -> String {
    format!("elephc-nightly-{target}.tar.gz")
}

pub fn nightly_checksum_name(target: &str) -> String {
    format!("{}.sha256", nightly_tarball_name(target))
}

fn api_base() -> String {
    match std::env::var("ELVM_GITHUB_API") {
        Ok(value) if !value.is_empty() => value,
        _ => DEFAULT_API.to_string(),
    }
}

pub fn parse_releases(json: &str) -> anyhow::Result<Vec<Release>> {
    let raw: Vec<RawRelease> = serde_json::from_str(json)?;
    let mut releases = Vec::new();
    for entry in raw {
        // Prereleases and drafts are not versions elvm should ever pick for
        // `latest`: semver ranks `0.27.0-beta.1` above `0.26.4` (a
        // prerelease marker only lowers a version relative to its own
        // final release, not relative to earlier finals), so leaving these
        // in would make `elvm install latest` silently start installing a
        // beta the moment one is published.
        if entry.prerelease || entry.draft {
            continue;
        }
        let body = entry.tag_name.trim_start_matches('v');
        // Tags that are not semver (a moving "nightly", say) are not versions
        // elvm can install, so they are skipped rather than treated as errors.
        let Ok(version) = Version::parse(body) else {
            continue;
        };
        releases.push(Release {
            version,
            assets: entry
                .assets
                .into_iter()
                .map(|a| Asset {
                    name: a.name,
                    url: a.browser_download_url,
                })
                .collect(),
        });
    }
    releases.sort_by(|a, b| b.version.cmp(&a.version));
    Ok(releases)
}

/// Parses the nightly entries out of the same release list, newest first.
///
/// Only the dated tags come back; the rolling `nightly` tag is deliberately
/// left out, because it duplicates the newest dated build's artifacts and
/// listing it beside them would show the same build twice. Callers that want
/// the rolling channel ask for it by tag with `fetch_nightly`.
///
/// Ordered by `published_at`, never by tag name: `nightly-20260901.10` sorts
/// before `nightly-20260901.2` lexicographically, which would name the wrong
/// build as the newest.
pub fn parse_nightlies(json: &str) -> anyhow::Result<Vec<NightlyRelease>> {
    let raw: Vec<RawRelease> = serde_json::from_str(json)?;
    let mut nightlies: Vec<NightlyRelease> = raw
        .into_iter()
        .filter(|entry| !entry.draft)
        .filter(|entry| matches!(Channel::parse(&entry.tag_name), Some(Channel::Dated(_))))
        .map(into_nightly)
        .collect();
    nightlies.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    Ok(nightlies)
}

/// Upstream names every nightly release `Nightly <version>`; the tag is the
/// fallback so a renamed release degrades to a less informative listing
/// rather than an empty version.
fn into_nightly(entry: RawRelease) -> NightlyRelease {
    let version = entry
        .name
        .strip_prefix("Nightly ")
        .filter(|v| !v.is_empty())
        .unwrap_or(&entry.tag_name)
        .to_string();
    NightlyRelease {
        tag: entry.tag_name,
        version,
        commit: entry.target_commitish,
        published_at: entry.published_at,
        assets: entry
            .assets
            .into_iter()
            .map(|a| Asset {
                name: a.name,
                url: a.browser_download_url,
            })
            .collect(),
    }
}

/// Requests per page, and the hard cap on pages fetched.
///
/// The cap is a backstop against a misbehaving API returning full pages
/// forever, not a real limit: 1000 releases is far beyond what elephc has
/// published, or is likely to for a long time.
const PER_PAGE: u32 = 100;
const MAX_PAGES: u32 = 10;

fn client() -> anyhow::Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("elvm/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Unauthenticated GitHub allows 60 requests per hour per IP, which CI runners
/// on shared egress do exhaust. A 403 is reported with the reset time and the
/// variable that raises the limit, rather than as a bare HTTP status.
fn rate_limit_error(response: &reqwest::blocking::Response) -> Option<anyhow::Error> {
    if response.status() != reqwest::StatusCode::FORBIDDEN {
        return None;
    }
    let reset = response
        .headers()
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    Some(anyhow::anyhow!(
        "GitHub rate limit exhausted (resets at unix time {reset})\n  \
         set GITHUB_TOKEN or GH_TOKEN to raise the limit"
    ))
}

/// Every release as one JSON array, served from cache when it is fresh.
///
/// GitHub paginates at 100 releases per page; elephc has published more than
/// that, so a single request silently drops the oldest releases. This fetches
/// pages until one comes back short (or empty), merging them into one JSON
/// array before caching — so each parser above sees exactly the shape it
/// would have seen from a single response, just assembled from several.
///
/// Both parsers read this one payload, so listing nightlies alongside
/// releases costs no extra request.
fn fetch_releases_json(paths: &ElvmPaths, refresh: bool) -> anyhow::Result<String> {
    let cache = paths.releases_json();
    if !refresh {
        if let Ok(metadata) = std::fs::metadata(&cache) {
            if let Ok(age) = metadata.modified().and_then(|m| {
                m.elapsed()
                    .map_err(|_| std::io::Error::other("clock moved backwards"))
            }) {
                if age < CACHE_TTL {
                    if let Ok(text) = std::fs::read_to_string(&cache) {
                        if serde_json::from_str::<Vec<RawRelease>>(&text).is_ok() {
                            return Ok(text);
                        }
                    }
                }
            }
        }
    }

    let client = client()?;
    let token = github_token();

    let mut entries: Vec<serde_json::Value> = Vec::new();
    for page in 1..=MAX_PAGES {
        let url = format!(
            "{}/repos/{REPO}/releases?per_page={PER_PAGE}&page={page}",
            api_base()
        );
        let mut request = client.get(&url);
        if let Some(token) = &token {
            request = request.bearer_auth(token);
        }

        let response = request.send()?;
        if let Some(err) = rate_limit_error(&response) {
            return Err(err);
        }
        let body = response.error_for_status()?.text()?;
        let page_entries: Vec<serde_json::Value> = serde_json::from_str(&body)?;
        let page_len = page_entries.len();
        entries.extend(page_entries);
        if page_len < PER_PAGE as usize {
            break;
        }
    }

    let merged = serde_json::to_string(&serde_json::Value::Array(entries))?;
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, &merged);
    Ok(merged)
}

/// Lists releases, serving a cached response when it is fresh enough.
pub fn list_releases(paths: &ElvmPaths, refresh: bool) -> anyhow::Result<Vec<Release>> {
    parse_releases(&fetch_releases_json(paths, refresh)?)
}

/// Lists the dated nightlies, newest first, from the same cached payload as
/// `list_releases`.
pub fn list_nightlies(paths: &ElvmPaths, refresh: bool) -> anyhow::Result<Vec<NightlyRelease>> {
    parse_nightlies(&fetch_releases_json(paths, refresh)?)
}

/// Fetches one nightly by tag, or `None` when that tag is not published.
///
/// Asked for by tag rather than found in the release list for two reasons:
/// it is a single unpaginated request, and it is never served from the cache
/// — which matters for the rolling tag, whose whole purpose is to have moved
/// since last time.
///
/// A missing tag is `None`, not an error: the workflow skips nights when
/// `main` is red or unchanged, so "no nightly right now" is a normal state,
/// and dated tags are pruned once they leave the retention window.
pub fn fetch_nightly(tag: &str) -> anyhow::Result<Option<NightlyRelease>> {
    let url = format!("{}/repos/{REPO}/releases/tags/{tag}", api_base());
    let mut request = client()?.get(&url);
    if let Some(token) = github_token() {
        request = request.bearer_auth(token);
    }

    let response = request.send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if let Some(err) = rate_limit_error(&response) {
        return Err(err);
    }
    let body = response.error_for_status()?.text()?;
    let entry: RawRelease = serde_json::from_str(&body)?;
    Ok(Some(into_nightly(entry)))
}

fn github_token() -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_upstream_asset_name() {
        let version = Version::parse("0.26.4").unwrap();
        assert_eq!(
            tarball_name(&version, "aarch64-apple-darwin"),
            "elephc-v0.26.4-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            checksum_name(&version, "aarch64-apple-darwin"),
            "elephc-v0.26.4-aarch64-apple-darwin.tar.gz.sha256"
        );
    }

    #[test]
    fn parses_releases_and_skips_unparseable_tags() {
        let json = r#"[
          {"tag_name":"v0.26.4","assets":[
            {"name":"elephc-v0.26.4-aarch64-apple-darwin.tar.gz","browser_download_url":"https://example.test/a"}
          ]},
          {"tag_name":"nightly","assets":[]}
        ]"#;
        let releases = parse_releases(json).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, Version::parse("0.26.4").unwrap());
        assert_eq!(releases[0].assets[0].url, "https://example.test/a");
    }

    #[test]
    fn prereleases_and_drafts_are_never_returned() {
        // The prerelease outranks the stable release by raw semver
        // (0.27.0-beta.1 > 0.26.4), so this also proves the filter runs
        // before anything downstream could pick it as "latest".
        let json = r#"[
          {"tag_name":"v0.27.0-beta.1","assets":[],"prerelease":true},
          {"tag_name":"v0.26.4","assets":[]},
          {"tag_name":"v0.28.0","assets":[],"draft":true}
        ]"#;
        let releases = parse_releases(json).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, Version::parse("0.26.4").unwrap());
    }

    #[test]
    fn nightly_asset_names_carry_no_version() {
        assert_eq!(
            nightly_tarball_name("aarch64-apple-darwin"),
            "elephc-nightly-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            nightly_checksum_name("aarch64-apple-darwin"),
            "elephc-nightly-aarch64-apple-darwin.tar.gz.sha256"
        );
    }

    #[test]
    fn parses_dated_nightlies_newest_first_and_leaves_out_the_rolling_tag() {
        let json = r#"[
          {"tag_name":"nightly","name":"Nightly 0.26.5-nightly.20260903+gccc",
           "target_commitish":"ccc","published_at":"2026-09-03T03:40:00Z","prerelease":true,"assets":[]},
          {"tag_name":"nightly-20260901.2","name":"Nightly 0.26.5-nightly.20260901+gbbb",
           "target_commitish":"bbb","published_at":"2026-09-01T18:00:00Z","prerelease":true,"assets":[]},
          {"tag_name":"nightly-20260901.10","name":"Nightly 0.26.5-nightly.20260901+gddd",
           "target_commitish":"ddd","published_at":"2026-09-01T21:00:00Z","prerelease":true,"assets":[]},
          {"tag_name":"nightly-20260903","name":"Nightly 0.26.5-nightly.20260903+gccc",
           "target_commitish":"ccc","published_at":"2026-09-03T03:40:00Z","prerelease":true,"assets":[]},
          {"tag_name":"v0.26.5","assets":[]}
        ]"#;
        let nightlies = parse_nightlies(json).unwrap();
        let tags: Vec<&str> = nightlies.iter().map(|n| n.tag.as_str()).collect();
        // Newest first, by publication time — by name, `.10` would sort
        // before `.2` and this order would be wrong.
        assert_eq!(
            tags,
            vec![
                "nightly-20260903",
                "nightly-20260901.10",
                "nightly-20260901.2"
            ]
        );
        assert_eq!(nightlies[0].version, "0.26.5-nightly.20260903+gccc");
        assert_eq!(nightlies[0].commit, "ccc");
    }

    #[test]
    fn nightlies_never_appear_among_releases() {
        // Both directions of the separation, on one payload: the nightly
        // tags are invisible to `parse_releases` (they are prereleases, and
        // their tags are not semver), and `v0.26.5` is invisible to
        // `parse_nightlies`.
        let json = r#"[
          {"tag_name":"nightly","prerelease":true,"assets":[]},
          {"tag_name":"nightly-20260901","prerelease":true,"assets":[]},
          {"tag_name":"v0.26.5","assets":[]}
        ]"#;
        let releases = parse_releases(json).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, Version::parse("0.26.5").unwrap());
        let nightlies = parse_nightlies(json).unwrap();
        assert_eq!(nightlies.len(), 1);
        assert_eq!(nightlies[0].tag, "nightly-20260901");
    }

    #[test]
    fn a_nightly_without_a_name_falls_back_to_its_tag() {
        let json = r#"[{"tag_name":"nightly-20260901","prerelease":true,"assets":[
          {"name":"elephc-nightly-x86_64-unknown-linux-gnu.tar.gz","browser_download_url":"https://example.test/a"},
          {"name":"elephc-nightly-aarch64-apple-darwin.tar.gz","browser_download_url":"https://example.test/b"},
          {"name":"elephc-nightly-aarch64-apple-darwin.tar.gz.sha256","browser_download_url":"https://example.test/c"}
        ]}]"#;
        let nightlies = parse_nightlies(json).unwrap();
        assert_eq!(nightlies[0].version, "nightly-20260901");
        // `.sha256` assets are not targets.
        assert_eq!(
            nightlies[0].targets(),
            vec!["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]
        );
    }

    #[test]
    fn sorts_releases_by_descending_semver_order() {
        // Deliberately give releases in mixed order, including a pair that
        // demonstrates semver vs lexicographic difference (0.9.0 vs 0.10.0).
        let json = r#"[
          {"tag_name":"v0.10.0","assets":[]},
          {"tag_name":"v0.26.4","assets":[]},
          {"tag_name":"v0.9.0","assets":[]},
          {"tag_name":"v1.0.0","assets":[]}
        ]"#;
        let releases = parse_releases(json).unwrap();
        assert_eq!(releases.len(), 4);
        // Must be strictly descending: 1.0.0, 0.26.4, 0.10.0, 0.9.0
        assert_eq!(releases[0].version, Version::parse("1.0.0").unwrap());
        assert_eq!(releases[1].version, Version::parse("0.26.4").unwrap());
        assert_eq!(releases[2].version, Version::parse("0.10.0").unwrap());
        assert_eq!(releases[3].version, Version::parse("0.9.0").unwrap());
    }
}
