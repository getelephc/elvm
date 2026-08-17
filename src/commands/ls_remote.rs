use crate::github;
use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::target;
use semver::Version;

/// One line of `ls-remote` output.
pub struct Row {
    pub version: Version,
    /// Patches older than this one within the same series; 0 when expanded.
    pub older_in_series: usize,
    /// True for the newest series, which is listed patch by patch.
    pub expanded: bool,
}

/// Groups published versions for display: the newest (major, minor) series
/// expanded patch by patch, every older series collapsed to its highest
/// patch with a count of the patches beneath it.
///
/// Pure: no filesystem or network access, so it is tested directly.
pub fn group(published: &[Version]) -> Vec<Row> {
    let mut sorted: Vec<Version> = published.to_vec();
    sorted.sort_by(|a, b| b.cmp(a));

    let Some(newest) = sorted.first().cloned() else {
        return Vec::new();
    };
    let newest_series = (newest.major, newest.minor);

    let mut rows = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let series = (sorted[i].major, sorted[i].minor);
        if series == newest_series {
            rows.push(Row {
                version: sorted[i].clone(),
                older_in_series: 0,
                expanded: true,
            });
            i += 1;
            continue;
        }

        let highest = sorted[i].clone();
        let mut count = 0usize;
        while i < sorted.len() && (sorted[i].major, sorted[i].minor) == series {
            count += 1;
            i += 1;
        }
        rows.push(Row {
            version: highest,
            older_in_series: count - 1,
            expanded: false,
        });
    }
    rows
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

/// The per-line note for a release missing a binary, suppressed when *no*
/// release has one — that case gets a single footer line instead (see
/// `run`), since repeating the same note on every line carries no
/// information.
fn platform_note(all_missing: bool, downloadable: bool) -> &'static str {
    if !all_missing && !downloadable {
        "  (no binary for this platform)"
    } else {
        ""
    }
}

pub fn run(paths: &ElvmPaths, all: bool) -> anyhow::Result<()> {
    let host = target::host()?;
    let releases = github::list_releases(paths, true)?;
    let installed = Installed::scan(paths)?;

    if releases.is_empty() {
        println!("no versions published upstream");
        return Ok(());
    }

    let total = releases.len();
    let downloadable_count = releases
        .iter()
        .filter(|r| r.asset(&github::tarball_name(&r.version, &host)).is_some())
        .count();
    let all_missing = downloadable_count == 0;

    let mut published: Vec<Version> = releases.iter().map(|r| r.version.clone()).collect();
    published.sort_by(|a, b| b.cmp(a));

    if all {
        for version in &published {
            let marker = if installed.versions.contains(version) {
                "*"
            } else {
                " "
            };
            let downloadable = has_binary(&releases, version, &host);
            let note = platform_note(all_missing, downloadable);
            println!("{marker} {version}{note}");
        }
        if all_missing {
            println!(
                "no elephc release publishes a binary for this platform — build it instead: elvm install --build <ref>"
            );
        }
        return Ok(());
    }

    let rows = group(&published);
    let mut printed_blank_before_collapsed = false;
    for (idx, row) in rows.iter().enumerate() {
        if !row.expanded && !printed_blank_before_collapsed {
            println!();
            printed_blank_before_collapsed = true;
        }
        let marker = if installed.versions.contains(&row.version) {
            "*"
        } else {
            " "
        };
        let downloadable = has_binary(&releases, &row.version, &host);
        let note = platform_note(all_missing, downloadable);
        let count_suffix = if row.older_in_series > 0 {
            format!("  (+{})", row.older_in_series)
        } else {
            String::new()
        };
        let latest_suffix = if idx == 0 { "  ← latest" } else { "" };
        println!(
            "{marker} {}{count_suffix}{note}{latest_suffix}",
            row.version
        );
    }

    println!();
    if all_missing {
        println!(
            "no elephc release publishes a binary for this platform — build it instead: elvm install --build <ref>"
        );
    }
    println!(
        "{} rows, {total} releases — elvm ls-remote --all for every patch",
        rows.len()
    );

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
        let rows = group(&published);
        assert_eq!(rows.len(), 5);
        for row in &rows {
            assert!(row.expanded);
            assert_eq!(row.older_in_series, 0);
        }
        assert_eq!(rows[0].version, v("0.26.4"));
        assert_eq!(rows[4].version, v("0.26.0"));
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
        let rows = group(&published);
        // 0.26.x expanded (2 rows) + 0.25 collapsed (1 row) + 0.24 collapsed (1 row)
        assert_eq!(rows.len(), 4);

        assert!(rows[0].expanded);
        assert_eq!(rows[0].version, v("0.26.4"));
        assert!(rows[1].expanded);
        assert_eq!(rows[1].version, v("0.26.3"));

        assert!(!rows[2].expanded);
        assert_eq!(rows[2].version, v("0.25.2"));
        // 3 patches in the 0.25 series (0.25.0, 0.25.1, 0.25.2) minus 1 = 2
        assert_eq!(rows[2].older_in_series, 2);

        assert!(!rows[3].expanded);
        assert_eq!(rows[3].version, v("0.24.0"));
        // Only one patch in the 0.24 series, so nothing beneath it.
        assert_eq!(rows[3].older_in_series, 0);
    }

    #[test]
    fn input_order_does_not_matter_output_is_always_descending() {
        // Deliberately unsorted, including a lexicographic trap (0.9 vs 0.10).
        let published = [v("0.9.0"), v("1.0.0"), v("0.10.0"), v("0.26.4")];
        let rows = group(&published);
        let versions: Vec<Version> = rows.iter().map(|r| r.version.clone()).collect();
        assert_eq!(
            versions,
            vec![v("1.0.0"), v("0.26.4"), v("0.10.0"), v("0.9.0")]
        );
    }

    #[test]
    fn a_single_series_produces_only_expanded_rows() {
        let published = [v("0.26.2"), v("0.26.1"), v("0.26.0")];
        let rows = group(&published);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.expanded));
        assert!(rows.iter().all(|r| r.older_in_series == 0));
    }

    #[test]
    fn empty_input_yields_no_rows() {
        let rows = group(&[]);
        assert!(rows.is_empty());
    }
}
