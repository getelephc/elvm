use crate::paths::ElvmPaths;
use semver::Version;
use serde::Deserialize;

/// A published elephc release, reduced to what elvm needs.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
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
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
}

impl Release {
    #[allow(dead_code)]
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

#[allow(dead_code)]
pub fn tarball_name(version: &Version, target: &str) -> String {
    format!("elephc-v{version}-{target}.tar.gz")
}

#[allow(dead_code)]
pub fn checksum_name(version: &Version, target: &str) -> String {
    format!("{}.sha256", tarball_name(version, target))
}

fn api_base() -> String {
    match std::env::var("ELVM_GITHUB_API") {
        Ok(value) if !value.is_empty() => value,
        _ => DEFAULT_API.to_string(),
    }
}

#[allow(dead_code)]
pub fn parse_releases(json: &str) -> anyhow::Result<Vec<Release>> {
    let raw: Vec<RawRelease> = serde_json::from_str(json)?;
    let mut releases = Vec::new();
    for entry in raw {
        let body = entry.tag_name.trim_start_matches('v');
        // Tags that are not semver (a moving "nightly", say) are not versions
        // elvm can install, so they are skipped rather than treated as errors.
        let Ok(version) = Version::parse(body) else {
            continue;
        };
        releases.push(Release {
            tag: entry.tag_name.clone(),
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

/// Lists releases, serving a cached response when it is fresh enough.
///
/// Unauthenticated GitHub allows 60 requests per hour per IP, which CI runners
/// on shared egress do exhaust, so responses are cached and a token is sent
/// when the environment provides one.
#[allow(dead_code)]
pub fn list_releases(paths: &ElvmPaths, refresh: bool) -> anyhow::Result<Vec<Release>> {
    let cache = paths.releases_json();
    if !refresh {
        if let Ok(metadata) = std::fs::metadata(&cache) {
            if let Ok(age) = metadata.modified().and_then(|m| {
                m.elapsed()
                    .map_err(|_| std::io::Error::other("clock moved backwards"))
            }) {
                if age < CACHE_TTL {
                    if let Ok(text) = std::fs::read_to_string(&cache) {
                        if let Ok(releases) = parse_releases(&text) {
                            return Ok(releases);
                        }
                    }
                }
            }
        }
    }

    let url = format!("{}/repos/{REPO}/releases?per_page=100", api_base());
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("elvm/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let mut request = client.get(&url);
    if let Some(token) = github_token() {
        request = request.bearer_auth(token);
    }

    let response = request.send()?;
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        let reset = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");
        anyhow::bail!(
            "GitHub rate limit exhausted (resets at unix time {reset})\n  \
             set GITHUB_TOKEN or GH_TOKEN to raise the limit"
        );
    }
    let body = response.error_for_status()?.text()?;
    let releases = parse_releases(&body)?;

    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, &body);
    Ok(releases)
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
