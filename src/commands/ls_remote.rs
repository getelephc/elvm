use crate::github;
use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::table;
use crate::target;
use semver::Version;

/// Shown when no release listed has a downloadable binary for this
/// platform. The `BINARY` column already says so per release; this adds the
/// one thing the column cannot, which is what to do about it.
const NO_BINARY_FOR_PLATFORM: &str =
    "none publishes a binary for this platform — build one instead: elvm install --build <ref>";

/// Maximum number of (major, minor) series the grouped view displays: the
/// newest series (expanded patch by patch) plus this many of the next most
/// recent, each collapsed to one row. One edit here moves the cutoff.
const MAX_SERIES: usize = 10;

/// One line of `ls-remote` output.
pub struct Row {
    pub version: Version,
    /// Patches older than this one within the same series; 0 when expanded.
    pub older_in_series: usize,
    /// True for the newest series, which is listed patch by patch.
    ///
    /// The table no longer renders collapsed and expanded rows differently
    /// — `OLDER` says how many patches a row stands for, which is the only
    /// distinction a reader needs — so nothing outside the tests reads this.
    /// It stays because it is what `group` decides, and the only thing that
    /// can tell a collapsed single-patch series from an individually listed
    /// release: `older_in_series` is 0 for both.
    #[allow(dead_code)]
    pub expanded: bool,
}

/// The result of grouping published versions for display.
pub struct Grouped {
    /// Rows in display order: the oldest shown series first, collapsed and
    /// ascending, then the newest series expanded patch by patch, also
    /// ascending — so the very last row is always the newest published
    /// version.
    pub rows: Vec<Row>,
    /// How many (major, minor) series exist in the input, shown or not.
    pub total_series: usize,
    /// Series older than what's displayed, dropped by the `MAX_SERIES`
    /// trim. Zero when there are `MAX_SERIES` series or fewer, i.e.
    /// nothing was actually hidden.
    pub hidden_series: usize,
}

/// Groups published versions for display: the newest (major, minor) series
/// expanded patch by patch, the next `MAX_SERIES - 1` older series each
/// collapsed to their highest patch with a count of the patches beneath it,
/// and anything older than that dropped (with `hidden_series` naming how
/// much).
///
/// Rows come back already in display order — oldest shown series first,
/// newest last — so the caller does no reordering of its own.
///
/// Pure: no filesystem or network access, so it is tested directly.
pub fn group(published: &[Version]) -> Grouped {
    let mut sorted: Vec<Version> = published.to_vec();
    sorted.sort();

    let Some(newest) = sorted.last().cloned() else {
        return Grouped {
            rows: Vec::new(),
            total_series: 0,
            hidden_series: 0,
        };
    };
    let newest_series = (newest.major, newest.minor);

    // Semver compares (major, minor) before patch, so an ascending sort
    // already leaves every series' patches contiguous; this just splits
    // those runs into per-series groups, oldest series first.
    let mut series: Vec<Vec<Version>> = Vec::new();
    for v in sorted {
        match series.last_mut() {
            Some(group) if (group[0].major, group[0].minor) == (v.major, v.minor) => {
                group.push(v);
            }
            _ => series.push(vec![v]),
        }
    }

    let total_series = series.len();
    let hidden_series = total_series.saturating_sub(MAX_SERIES);
    let kept = if total_series > MAX_SERIES {
        series.split_off(total_series - MAX_SERIES)
    } else {
        series
    };

    let mut rows = Vec::new();
    for patches in kept {
        let series_key = (patches[0].major, patches[0].minor);
        if series_key == newest_series {
            rows.extend(patches.into_iter().map(|version| Row {
                version,
                older_in_series: 0,
                expanded: true,
            }));
        } else {
            let older_in_series = patches.len() - 1;
            let version = patches
                .into_iter()
                .next_back()
                .expect("series is non-empty");
            rows.push(Row {
                version,
                older_in_series,
                expanded: false,
            });
        }
    }

    Grouped {
        rows,
        total_series,
        hidden_series,
    }
}

/// Whether a release's own installed version has a binary for `host`, looked
/// up by linear scan — the release list is a couple hundred entries at most,
/// so this trades a HashMap for not needing `Version: Hash`.
fn has_binary(releases: &[github::Release], version: &Version, host: &str) -> bool {
    releases
        .iter()
        .find(|r| &r.version == version)
        .map(|r| r.asset(&github::tarball_name(version, host)).is_some())
        .unwrap_or(false)
}

/// Dated nightlies shown in the grouped view. `--all` lists every one, the
/// same way it expands every patch.
const MAX_NIGHTLIES: usize = 5;

/// Column headings for the release table. `OLDER` counts the patches a
/// collapsed row stands for; `BINARY` says whether this platform can
/// download that release at all — a fact that used to be repeated as a
/// sentence on every line that lacked one.
const RELEASE_HEADERS: [&str; 4] = ["VERSION", "OLDER", "BINARY", "NOTE"];

/// Column headings for the nightly table.
const NIGHTLY_HEADERS: [&str; 4] = ["NIGHTLY", "BUILD", "PUBLISHED", "NOTE"];

/// Prints the dated nightlies, newest first, above the releases.
///
/// Ordering is no longer load-bearing: the newest release used to have to be
/// the last line on screen because nothing else identified it, and now its
/// row says `latest` outright. So this sits where it reads best rather than
/// where it has to.
///
/// Reads the payload `run` has just refreshed, so this costs no extra
/// request. The order comes from `list_nightlies`, which sorts by
/// publication time — by tag name `nightly-20260901.10` would sort before
/// `nightly-20260901.2` and the wrong build would be named newest.
fn print_nightlies(
    paths: &ElvmPaths,
    installed: &Installed,
    host: &str,
    all: bool,
) -> anyhow::Result<()> {
    let nightlies = github::list_nightlies(paths, false)?;

    if nightlies.is_empty() {
        // Only the *dated* tags are listed here, so an empty list does not
        // mean the channel is empty: the rolling tag may well have a build,
        // and `elvm install nightly` would install it. Saying "none
        // published" would talk someone out of a command that works.
        println!("no dated nightly to pin; elvm install nightly takes the newest build of main");
        return Ok(());
    }

    let shown = if all {
        nightlies.len()
    } else {
        nightlies.len().min(MAX_NIGHTLIES)
    };
    let hidden = nightlies.len() - shown;
    let more = if hidden > 0 {
        format!(", {hidden} older hidden")
    } else {
        String::new()
    };
    let count = nightlies.len();
    let plural = if count == 1 { "" } else { "s" };
    println!(
        "{count} unsupported nightly build{plural} of main{more} — elvm install nightly takes the newest"
    );

    let rows: Vec<Vec<String>> = nightlies[..shown]
        .iter()
        .map(|nightly| {
            let mut notes = Vec::new();
            if installed.aliases.contains(&nightly.tag) {
                notes.push("installed");
            }
            if nightly.asset(&github::nightly_tarball_name(host)).is_none() {
                notes.push("no binary for this platform");
            }
            vec![
                nightly.tag.clone(),
                nightly.version.clone(),
                nightly
                    .published_at
                    .split('T')
                    .next()
                    .unwrap_or_default()
                    .to_string(),
                notes.join(", "),
            ]
        })
        .collect();
    table::print(&NIGHTLY_HEADERS, &rows);
    Ok(())
}

pub fn run(paths: &ElvmPaths, all: bool) -> anyhow::Result<()> {
    let host = target::host()?;
    let releases = github::list_releases(paths, true)?;
    let installed = Installed::scan(paths)?;

    if releases.is_empty() {
        println!("no versions published upstream");
        return Ok(());
    }

    print_nightlies(paths, &installed, &host, all)?;
    println!();

    let mut published: Vec<Version> = releases.iter().map(|r| r.version.clone()).collect();
    // Ascending: oldest first, so the newest release is the table's last row.
    published.sort();

    // `BINARY` answers per release, from the release's own assets rather
    // than a fixed platform list — which is the same question
    // `install::no_binary_error` asks, and the reason neither consults a
    // constant.
    let binary_for = |version: &Version| {
        if has_binary(&releases, version, &host) {
            "yes"
        } else {
            "no"
        }
    };
    let note_for = |version: &Version, latest: bool| {
        let mut notes = Vec::new();
        if latest {
            notes.push("latest");
        }
        if installed.versions.contains(version) {
            notes.push("installed");
        }
        notes.join(", ")
    };

    let rows: Vec<Vec<String>> = if all {
        println!("{} releases, every published patch", releases.len());
        let last_idx = published.len().saturating_sub(1);
        published
            .iter()
            .enumerate()
            .map(|(idx, version)| {
                vec![
                    version.to_string(),
                    String::new(),
                    binary_for(version).to_string(),
                    note_for(version, idx == last_idx),
                ]
            })
            .collect()
    } else {
        let grouped = group(&published);
        let hidden = if grouped.hidden_series > 0 {
            format!(", {} older hidden", grouped.hidden_series)
        } else {
            String::new()
        };
        println!(
            "{} releases in {} series{hidden} — elvm ls-remote --all for every patch",
            releases.len(),
            grouped.total_series
        );
        let last_idx = grouped.rows.len().saturating_sub(1);
        grouped
            .rows
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let older = if row.older_in_series > 0 {
                    format!("+{}", row.older_in_series)
                } else {
                    String::new()
                };
                vec![
                    row.version.to_string(),
                    older,
                    binary_for(&row.version).to_string(),
                    note_for(&row.version, idx == last_idx),
                ]
            })
            .collect()
    };

    // A `BINARY` column reading `no` all the way down says what is missing
    // but not what to do about it, and this is the one case where the answer
    // is not "install a different version".
    if rows.iter().all(|row| row[2] == "no") {
        println!("{NO_BINARY_FOR_PLATFORM}");
    }
    table::print(&RELEASE_HEADERS, &rows);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn newest_series_is_expanded_patch_by_patch() {
        let published = [
            v("0.26.4"),
            v("0.26.3"),
            v("0.26.2"),
            v("0.26.1"),
            v("0.26.0"),
        ];
        let grouped = group(&published);
        assert_eq!(grouped.rows.len(), 5);
        for row in &grouped.rows {
            assert!(row.expanded);
            assert_eq!(row.older_in_series, 0);
        }
        // Ascending: oldest first, newest (== highest) last.
        assert_eq!(grouped.rows[0].version, v("0.26.0"));
        assert_eq!(grouped.rows[4].version, v("0.26.4"));
    }

    #[test]
    fn older_series_collapse_with_a_correct_patch_count() {
        let published = [
            v("0.26.4"),
            v("0.26.3"),
            v("0.25.2"),
            v("0.25.1"),
            v("0.25.0"),
            v("0.24.0"),
        ];
        let grouped = group(&published);
        // 0.24 collapsed (1 row) + 0.25 collapsed (1 row) + 0.26.x expanded (2 rows)
        assert_eq!(grouped.rows.len(), 4);

        assert!(!grouped.rows[0].expanded);
        assert_eq!(grouped.rows[0].version, v("0.24.0"));
        // Only one patch in the 0.24 series, so nothing beneath it.
        assert_eq!(grouped.rows[0].older_in_series, 0);

        assert!(!grouped.rows[1].expanded);
        assert_eq!(grouped.rows[1].version, v("0.25.2"));
        // 3 patches in the 0.25 series (0.25.0, 0.25.1, 0.25.2) minus 1 = 2
        assert_eq!(grouped.rows[1].older_in_series, 2);

        assert!(grouped.rows[2].expanded);
        assert_eq!(grouped.rows[2].version, v("0.26.3"));
        assert!(grouped.rows[3].expanded);
        assert_eq!(grouped.rows[3].version, v("0.26.4"));
    }

    #[test]
    fn input_order_does_not_matter_output_is_always_ascending() {
        // Deliberately unsorted, including a lexicographic trap (0.9 vs 0.10):
        // 0.9.0 must sort below 0.10.0 by semver, not above it as a string
        // comparison would put it.
        let published = [v("0.9.0"), v("1.0.0"), v("0.10.0"), v("0.26.4")];
        let grouped = group(&published);
        let versions: Vec<Version> = grouped.rows.iter().map(|r| r.version.clone()).collect();
        assert_eq!(
            versions,
            vec![v("0.9.0"), v("0.10.0"), v("0.26.4"), v("1.0.0")]
        );
        // The newest (highest) version is always the final row.
        assert_eq!(grouped.rows.last().unwrap().version, v("1.0.0"));
    }

    #[test]
    fn a_9_1_series_still_sorts_below_a_10_0_series_at_the_boundary() {
        // Same trap as above, but pinned right at the two-series boundary
        // relevant to grouping/collapsing, with patch numbers that would
        // also collide under a naive string compare (0.9.10 > 0.10.0
        // lexicographically, but not by semver).
        let published = [v("0.10.0"), v("0.9.1"), v("0.9.10")];
        let grouped = group(&published);
        let versions: Vec<Version> = grouped.rows.iter().map(|r| r.version.clone()).collect();
        // 0.9.x collapses (2 patches -> 1 row), 0.10.0 is the newest series,
        // expanded.
        assert_eq!(grouped.rows.len(), 2);
        assert_eq!(versions, vec![v("0.9.10"), v("0.10.0")]);
        assert_eq!(grouped.rows[0].older_in_series, 1);
        assert!(!grouped.rows[0].expanded);
        assert!(grouped.rows[1].expanded);
    }

    #[test]
    fn a_single_series_produces_only_expanded_rows() {
        let published = [v("0.26.2"), v("0.26.1"), v("0.26.0")];
        let grouped = group(&published);
        assert_eq!(grouped.rows.len(), 3);
        assert!(grouped.rows.iter().all(|r| r.expanded));
        assert!(grouped.rows.iter().all(|r| r.older_in_series == 0));
        assert_eq!(grouped.rows.last().unwrap().version, v("0.26.2"));
        assert_eq!(grouped.hidden_series, 0);
    }

    #[test]
    fn empty_input_yields_no_rows() {
        let grouped = group(&[]);
        assert!(grouped.rows.is_empty());
        assert_eq!(grouped.total_series, 0);
        assert_eq!(grouped.hidden_series, 0);
    }

    /// One version per (major, minor) series, `n` series total, ascending:
    /// `0.1.0`, `0.2.0`, ..., `0.n.0`. Enough to drive the `MAX_SERIES` trim
    /// without needing a realistic patch spread.
    fn n_series(n: u64) -> Vec<Version> {
        (1..=n).map(|minor| v(&format!("0.{minor}.0"))).collect()
    }

    #[test]
    fn no_hidden_line_when_series_count_is_at_or_under_the_max() {
        for n in [1, 9, 10] {
            let grouped = group(&n_series(n));
            assert_eq!(grouped.hidden_series, 0, "n={n}");
            assert_eq!(grouped.rows.len() as u64, n, "n={n}");
        }
    }

    #[test]
    fn hidden_line_appears_as_soon_as_series_count_exceeds_the_max() {
        let grouped = group(&n_series(11));
        assert_eq!(grouped.total_series, 11);
        assert_eq!(grouped.hidden_series, 1);
        assert_eq!(grouped.rows.len(), 10);
    }

    #[test]
    fn hidden_count_is_exact_when_series_count_exceeds_the_max() {
        let grouped = group(&n_series(24));
        assert_eq!(grouped.total_series, 24);
        // MAX_SERIES (10) shown: 9 collapsed + 1 expanded.
        assert_eq!(grouped.hidden_series, 14);
        assert_eq!(grouped.rows.len(), 10);

        // The 10 series shown (MAX_SERIES) are the most recent ones: series
        // 15..=24, ascending; series 24 is the newest and is expanded
        // instead of collapsed.
        let shown: Vec<Version> = grouped.rows.iter().map(|r| r.version.clone()).collect();
        let expected: Vec<Version> = (15..=24).map(|minor| v(&format!("0.{minor}.0"))).collect();
        assert_eq!(shown, expected);
        assert!(grouped.rows.last().unwrap().expanded);
        assert_eq!(grouped.rows.last().unwrap().version, v("0.24.0"));
    }
}
