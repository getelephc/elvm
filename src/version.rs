use crate::nightly::Channel;
use semver::Version;

/// A version as requested by a user, a `.elephc-version` file, or the global file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionRequest {
    /// A fully specified version: `0.26.4`.
    Exact(Version),
    /// A leading fragment: `0.26` matches the highest installed `0.26.x`.
    Prefix(String),
    /// Highest installed version. In `install`, the caller treats this as
    /// "newest published" instead — see the spec's asymmetry rule.
    Latest,
    /// A nightly: the rolling `nightly` channel, or a dated build like
    /// `nightly-20260901`. Kept apart from `Exact` because a nightly's
    /// version string, though valid semver, sorts *below* the release it
    /// derives from — see `crate::nightly`.
    Nightly(Channel),
    /// A directory name in `versions/`, such as `dev` from `elvm link`.
    Alias(String),
}

impl VersionRequest {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let body = trimmed.strip_prefix('v').unwrap_or(trimmed);

        if body.eq_ignore_ascii_case("latest") {
            return Self::Latest;
        }
        // Checked against `trimmed`, not `body`: a nightly tag never carries
        // the `v` prefix that stripping produces.
        if let Some(channel) = Channel::parse(trimmed) {
            return Self::Nightly(channel);
        }
        if let Ok(version) = Version::parse(body) {
            return Self::Exact(version);
        }
        if !body.is_empty()
            && body
                .split('.')
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        {
            return Self::Prefix(body.to_string());
        }
        Self::Alias(trimmed.to_string())
    }

    /// Picks the best installed version for this request.
    ///
    /// Returns `None` for `Alias` and `Nightly`: both are directory names,
    /// resolved by `installed::resolve_name` rather than by semver
    /// comparison.
    pub fn select(&self, available: &[Version]) -> Option<Version> {
        match self {
            Self::Exact(wanted) => available.iter().find(|v| *v == wanted).cloned(),
            Self::Prefix(prefix) => available
                .iter()
                .filter(|v| {
                    let text = v.to_string();
                    text == *prefix || text.starts_with(&format!("{prefix}."))
                })
                .max()
                .cloned(),
            Self::Latest => available.iter().max().cloned(),
            Self::Nightly(_) | Self::Alias(_) => None,
        }
    }
}

impl std::fmt::Display for VersionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(v) => write!(f, "{v}"),
            Self::Prefix(p) => write!(f, "{p}"),
            Self::Latest => write!(f, "latest"),
            Self::Nightly(channel) => write!(f, "{channel}"),
            Self::Alias(a) => write!(f, "{a}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_exact_versions_with_or_without_v_prefix() {
        assert_eq!(
            VersionRequest::parse("0.26.4"),
            VersionRequest::Exact(v("0.26.4"))
        );
        assert_eq!(
            VersionRequest::parse("v0.26.4"),
            VersionRequest::Exact(v("0.26.4"))
        );
        assert_eq!(
            VersionRequest::parse(" 0.26.4\n"),
            VersionRequest::Exact(v("0.26.4"))
        );
    }

    #[test]
    fn parses_prefixes_and_latest_and_aliases() {
        assert_eq!(
            VersionRequest::parse("0.26"),
            VersionRequest::Prefix("0.26".into())
        );
        assert_eq!(
            VersionRequest::parse("v0"),
            VersionRequest::Prefix("0".into())
        );
        assert_eq!(VersionRequest::parse("latest"), VersionRequest::Latest);
        assert_eq!(
            VersionRequest::parse("dev"),
            VersionRequest::Alias("dev".into())
        );
    }

    #[test]
    fn parses_nightly_tags_as_a_channel_not_an_alias() {
        use crate::nightly::Channel;
        assert_eq!(
            VersionRequest::parse("nightly"),
            VersionRequest::Nightly(Channel::Rolling)
        );
        assert_eq!(
            VersionRequest::parse(" nightly-20260901\n"),
            VersionRequest::Nightly(Channel::Dated("nightly-20260901".into()))
        );
        // Not the published grammar: it stays an alias, so it fails as a
        // missing directory rather than being sent to the nightly endpoint.
        assert_eq!(
            VersionRequest::parse("nightly-2026-09-01"),
            VersionRequest::Alias("nightly-2026-09-01".into())
        );
    }

    #[test]
    fn a_nightly_never_selects_a_semver_version() {
        // The safety property: even though `0.26.5-nightly.20260901` is
        // valid semver, no nightly request can resolve to an installed
        // release, and no release request can resolve to a nightly.
        let available = [v("0.26.4"), v("0.26.5")];
        assert_eq!(VersionRequest::parse("nightly").select(&available), None);
        assert_eq!(
            VersionRequest::parse("nightly-20260901").select(&available),
            None
        );
    }

    #[test]
    fn exact_selects_only_that_version() {
        let available = [v("0.25.2"), v("0.26.4")];
        assert_eq!(
            VersionRequest::parse("0.26.4").select(&available),
            Some(v("0.26.4"))
        );
        assert_eq!(VersionRequest::parse("0.26.3").select(&available), None);
    }

    #[test]
    fn prefix_selects_highest_match_and_respects_component_boundaries() {
        let available = [v("0.26.1"), v("0.26.4"), v("0.260.1"), v("0.25.2")];
        assert_eq!(
            VersionRequest::parse("0.26").select(&available),
            Some(v("0.26.4"))
        );
        // "0.26" must not match "0.260.1"
        assert_eq!(
            VersionRequest::parse("0.260").select(&available),
            Some(v("0.260.1"))
        );
    }

    #[test]
    fn latest_selects_highest_installed_by_semver_order_not_string_order() {
        let available = [v("0.9.0"), v("0.10.0")];
        assert_eq!(
            VersionRequest::parse("latest").select(&available),
            Some(v("0.10.0"))
        );
    }

    #[test]
    fn alias_never_selects_a_semver_directory() {
        let available = [v("0.26.4")];
        assert_eq!(VersionRequest::parse("dev").select(&available), None);
    }

    #[test]
    fn selecting_from_nothing_yields_nothing() {
        assert_eq!(VersionRequest::parse("latest").select(&[]), None);
    }
}
