//! The nightly channel: unattended builds of elephc's `main`.
//!
//! Nightlies are deliberately kept out of the semver world the rest of elvm
//! lives in. Their version strings *are* semver — `0.26.5-nightly.20260901+g672ff38c5`
//! — but ordering them alongside releases is wrong twice over: a pre-release
//! sorts *below* the release it names (`0.26.5-nightly.20260901 < 0.26.5`)
//! even though the code is newer, and `+g<sha>` is build metadata, which
//! semver ignores in comparisons, so two builds from the same day compare
//! equal. So a nightly is addressed by tag, installed under that tag's name,
//! and never parsed as a `semver::Version` by elvm — which also keeps it out
//! of `latest` and prefix selection for free, since neither `nightly` nor
//! `nightly-20260901` parses as one.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The rolling tag upstream moves every night. It always names the newest
/// build, and names a different one from one day to the next.
pub const ROLLING_TAG: &str = "nightly";

/// Recorded beside the binary so elvm can tell which build is installed:
/// nothing in the directory's name says, because a rolling install is always
/// called `nightly`.
pub const STAMP_FILE: &str = ".elvm-nightly.json";

/// A nightly reference, from the command line or a `.elephc-version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// `nightly` — whichever build is newest at the moment it is installed.
    Rolling,
    /// `nightly-20260901`, or `nightly-20260901.2` for a second build on the
    /// same UTC day. Upstream never republishes a dated tag, so this is a
    /// real pin — for as long as the retention window keeps it (§ Retention
    /// in the README).
    Dated(String),
}

impl Channel {
    /// Recognises the tag grammar elephc's nightly workflow publishes:
    /// `nightly`, `nightly-<8 digits>`, or `nightly-<8 digits>.<n>`.
    ///
    /// The shape is validated rather than accepted loosely so that a typo
    /// (`nightly-2026-09-01`) fails as a version request — which the caller
    /// reports with the command that lists real tags — instead of becoming
    /// an `Alias` and failing later as a missing directory.
    pub fn parse(raw: &str) -> Option<Self> {
        if raw == ROLLING_TAG {
            return Some(Self::Rolling);
        }
        let rest = raw.strip_prefix("nightly-")?;
        let (date, suffix) = match rest.split_once('.') {
            Some((date, suffix)) => (date, Some(suffix)),
            None => (rest, None),
        };
        if date.len() != 8 || !date.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if let Some(suffix) = suffix {
            if suffix.is_empty() || !suffix.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
        }
        Some(Self::Dated(raw.to_string()))
    }

    /// The upstream tag this refers to.
    pub fn tag(&self) -> &str {
        match self {
            Self::Rolling => ROLLING_TAG,
            Self::Dated(tag) => tag,
        }
    }

    /// The directory under `versions/`. The tag doubles as the name: it is
    /// never valid semver, so `Installed::scan` files it as an alias and it
    /// stays invisible to `latest` and to prefix requests like `0.26`.
    pub fn dir_name(&self) -> &str {
        self.tag()
    }

    pub fn is_rolling(&self) -> bool {
        matches!(self, Self::Rolling)
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

/// What was installed, written into the version directory at install time.
///
/// `sha256` is the identity, not `version`: two builds from the same UTC day
/// carry the same version string apart from the `+g<sha>` build metadata,
/// which semver comparisons ignore. The digest of the tarball distinguishes
/// them, and is already fetched to verify the download, so it costs nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub tag: String,
    pub version: String,
    pub commit: String,
    pub published_at: String,
    pub sha256: String,
}

impl Stamp {
    /// Reads the stamp from a version directory, or `None` when there is
    /// none to read — an unstamped directory (hand-made, or from an elvm
    /// that predates the nightly channel) is not an error, it just cannot
    /// claim to be up to date.
    pub fn read(dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join(STAMP_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn write(&self, dir: &Path) -> anyhow::Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(dir.join(STAMP_FILE), text)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", dir.join(STAMP_FILE).display()))
    }

    /// `published_at` reduced to its date, for humans. Upstream sends
    /// RFC 3339 in UTC; anything else is shown whole rather than mangled.
    pub fn published_date(&self) -> &str {
        match self.published_at.split_once('T') {
            Some((date, _)) => date,
            None => &self.published_at,
        }
    }

    /// `0.26.5-nightly.20260901+g672ff38c5 (published 2026-09-01)`.
    ///
    /// The commit is not repeated: the version string already ends in
    /// `+g<short sha>`, and upstream lets git choose that abbreviation's
    /// length, so re-deriving one here would print a different string for
    /// the same commit.
    pub fn summary(&self) -> String {
        format!("{} (published {})", self.version, self.published_date())
    }

    /// Whole days between `published_at` and now, or `None` when the
    /// timestamp is not a date elvm can read. Used only to tell someone
    /// their nightly has aged out of the upstream retention window, so an
    /// unreadable timestamp simply means no such note.
    pub fn age_in_days(&self, now_unix: i64) -> Option<i64> {
        let date = self.published_at.split('T').next()?;
        let mut parts = date.split('-');
        let year: i64 = parts.next()?.parse().ok()?;
        let month: u32 = parts.next()?.parse().ok()?;
        let day: u32 = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(now_unix.div_euclid(86_400) - days_from_civil(year, month, day))
    }
}

/// Days from 1970-01-01 to `y-m-d`, by Howard Hinnant's civil-date algorithm.
///
/// Written out rather than pulled in: elvm needs one date subtraction, to
/// decide whether to print a staleness note, and a date library would be a
/// dependency (and an attack surface) for exactly that.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let d = d as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds since the Unix epoch, or 0 if the clock is before it — which only
/// makes every nightly look brand new, never stale, so a badly set clock
/// suppresses the staleness note rather than inventing one.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_rolling_tag_and_dated_tags() {
        assert_eq!(Channel::parse("nightly"), Some(Channel::Rolling));
        assert_eq!(
            Channel::parse("nightly-20260901"),
            Some(Channel::Dated("nightly-20260901".into()))
        );
        // A second build on the same UTC day: upstream never overwrites a
        // dated tag, it suffixes instead.
        assert_eq!(
            Channel::parse("nightly-20260901.2"),
            Some(Channel::Dated("nightly-20260901.2".into()))
        );
        assert_eq!(
            Channel::parse("nightly-20260901.10"),
            Some(Channel::Dated("nightly-20260901.10".into()))
        );
    }

    #[test]
    fn rejects_tags_that_are_not_the_published_grammar() {
        for raw in [
            "nightlies",
            "nightly-",
            "nightly-2026-09-01",
            "nightly-202609",
            "nightly-20260901.",
            "nightly-20260901.x",
            "nightly-20260901.2.3",
            "Nightly",
            "dev",
            "0.26.4",
        ] {
            assert_eq!(Channel::parse(raw), None, "{raw} must not parse");
        }
    }

    #[test]
    fn a_dated_tag_is_never_valid_semver() {
        // The property the whole design rests on: a nightly directory name
        // cannot be parsed as a version, so `Installed::scan` files it as an
        // alias and `latest`/`0.26` can never select it.
        for raw in ["nightly", "nightly-20260901", "nightly-20260901.2"] {
            assert!(
                semver::Version::parse(raw).is_err(),
                "{raw} must not parse as semver"
            );
        }
    }

    fn stamp(published_at: &str) -> Stamp {
        Stamp {
            tag: "nightly".into(),
            version: "0.26.5-nightly.20260901+g672ff38c5".into(),
            commit: "672ff38c5".into(),
            published_at: published_at.into(),
            sha256: "d1".into(),
        }
    }

    #[test]
    fn summarises_a_build_by_version_and_date() {
        assert_eq!(
            stamp("2026-09-01T15:20:11Z").summary(),
            "0.26.5-nightly.20260901+g672ff38c5 (published 2026-09-01)"
        );
    }

    #[test]
    fn measures_age_in_whole_days() {
        // 2026-09-15T00:00:00Z
        let now = days_from_civil(2026, 9, 15) * 86_400;
        assert_eq!(stamp("2026-09-15T03:20:11Z").age_in_days(now), Some(0));
        assert_eq!(stamp("2026-09-01T15:20:11Z").age_in_days(now), Some(14));
        // Across a leap day, where naive month arithmetic would drift.
        assert_eq!(
            stamp("2024-02-28T00:00:00Z").age_in_days(days_from_civil(2024, 3, 1) * 86_400),
            Some(2)
        );
    }

    #[test]
    fn an_unreadable_timestamp_yields_no_age() {
        for raw in ["", "yesterday", "2026-09", "2026-13-01T00:00:00Z"] {
            assert_eq!(stamp(raw).age_in_days(0), None, "{raw} must not parse");
        }
    }

    #[test]
    fn days_from_civil_matches_known_epochs() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
    }
}
