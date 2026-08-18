mod support;

use assert_cmd::Command;
use httpmock::prelude::*;
use support::Sandbox;

/// Builds a tarball with the same flat shape as an elephc release. The
/// `elephc` entry is marked executable (mode 0o755) so that assertions on
/// the installed binary's permission bits actually mean something.
fn fake_release_tarball() -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));

    let script = b"#!/bin/sh\necho \"fake-elephc $*\"\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, "elephc", &script[..])
        .unwrap();

    for archive in support::BRIDGE_ARCHIVES {
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, archive, &b""[..]).unwrap();
    }

    builder.into_inner().unwrap().finish().unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

struct Upstream {
    server: MockServer,
}

impl Upstream {
    /// Serves a releases list plus a tarball and matching checksum for one version.
    fn start(version: &str) -> Self {
        Self::start_many(&[version], None)
    }

    /// Like `start`, but the `.sha256` asset serves a digest that can never
    /// match the tarball, from the outset — so verification must fail before
    /// anything is extracted or installed.
    fn start_corrupt(version: &str) -> Self {
        Self::start_many(&[version], Some(format!("{}  x", "0".repeat(64))))
    }

    /// Serves a releases list with one matching tarball+checksum pair per
    /// entry in `versions`, all sharing the same fake tarball content. Used
    /// to pin that `install latest` picks the highest *published* version
    /// among several, not merely "the only one available".
    fn start_many(versions: &[&str], bad_checksum: Option<String>) -> Self {
        let server = MockServer::start();
        let tarball = fake_release_tarball();
        let mut entries = Vec::new();

        for version in versions {
            let name = format!("elephc-v{version}-{}", elvm_target());
            let tar_name = format!("{name}.tar.gz");
            let tar_url = server.url(format!("/download/{tar_name}"));
            let sha_url = server.url(format!("/download/{tar_name}.sha256"));

            entries.push(format!(
                r#"{{"tag_name":"v{version}","assets":[
                    {{"name":"{tar_name}","browser_download_url":"{tar_url}"}},
                    {{"name":"{tar_name}.sha256","browser_download_url":"{sha_url}"}}
                ]}}"#,
            ));

            server.mock(|when, then| {
                when.method(GET).path(format!("/download/{tar_name}"));
                then.status(200).body(tarball.clone());
            });

            let checksum_body = match &bad_checksum {
                Some(digest) => format!("{digest}\n"),
                None => format!("{}  {tar_name}\n", sha256_hex(&tarball)),
            };
            server.mock(|when, then| {
                when.method(GET)
                    .path(format!("/download/{tar_name}.sha256"));
                then.status(200).body(checksum_body.clone());
            });
        }

        let releases = format!("[{}]", entries.join(","));
        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/illegalstudio/elephc/releases");
            then.status(200)
                .header("content-type", "application/json")
                .body(releases);
        });

        Self { server }
    }

    /// Serves a releases list plus a tarball and matching checksum for one
    /// version, with assets named for an explicit `target` rather than
    /// `elvm_target()`. Used to prove a release that actually carries the
    /// host's asset installs cleanly on a non-macOS host, without touching
    /// every other fixture's default (macOS) asset naming.
    fn start_for_target(version: &str, target: &str) -> Self {
        let server = MockServer::start();
        let tarball = fake_release_tarball();

        let tar_name = format!("elephc-v{version}-{target}.tar.gz");
        let tar_url = server.url(format!("/download/{tar_name}"));
        let sha_url = server.url(format!("/download/{tar_name}.sha256"));

        server.mock(|when, then| {
            when.method(GET).path(format!("/download/{tar_name}"));
            then.status(200).body(tarball.clone());
        });
        server.mock(|when, then| {
            when.method(GET)
                .path(format!("/download/{tar_name}.sha256"));
            then.status(200)
                .body(format!("{}  {tar_name}\n", sha256_hex(&tarball)));
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/illegalstudio/elephc/releases");
            then.status(200)
                .header("content-type", "application/json")
                .body(format!(
                    r#"[{{"tag_name":"v{version}","assets":[
                        {{"name":"{tar_name}","browser_download_url":"{tar_url}"}},
                        {{"name":"{tar_name}.sha256","browser_download_url":"{sha_url}"}}
                    ]}}]"#,
                ));
        });

        Self { server }
    }

    /// Serves two versions: one with the current target's tarball, one with
    /// only a different platform's assets. Tests the "no binary for this
    /// platform" marker in ls-remote.
    fn start_with_missing_platform(available: &str, unavailable: &str) -> Self {
        let server = MockServer::start();
        let tarball = fake_release_tarball();
        let mut entries = Vec::new();

        // Version with current platform available
        let available_name = format!("elephc-v{}-{}", available, elvm_target());
        let available_tar = format!("{}.tar.gz", available_name);
        let available_url = server.url(format!("/download/{}", available_tar));
        let available_sha_url = server.url(format!("/download/{}.sha256", available_tar));

        entries.push(format!(
            r#"{{"tag_name":"v{}","assets":[
                {{"name":"{}","browser_download_url":"{}"}},
                {{"name":"{}.sha256","browser_download_url":"{}"}}
            ]}}"#,
            available, available_tar, available_url, available_tar, available_sha_url
        ));

        server.mock(|when, then| {
            when.method(GET)
                .path(format!("/download/{}", available_tar));
            then.status(200).body(tarball.clone());
        });

        server.mock(|when, then| {
            when.method(GET)
                .path(format!("/download/{}.sha256", available_tar));
            then.status(200)
                .body(format!("{}  {}\n", sha256_hex(&tarball), available_tar));
        });

        // Version with a different platform only (no current target)
        let unavailable_name = format!("elephc-v{}-x86_64-unknown-linux-gnu", unavailable);
        let unavailable_tar = format!("{}.tar.gz", unavailable_name);

        entries.push(format!(
            r#"{{"tag_name":"v{}","assets":[{{"name":"{}","browser_download_url":"/missing"}}]}}"#,
            unavailable, unavailable_tar
        ));

        let releases = format!("[{}]", entries.join(","));
        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/illegalstudio/elephc/releases");
            then.status(200)
                .header("content-type", "application/json")
                .body(releases);
        });

        Self { server }
    }
}

/// The default fixture target: most tests exercise the download path
/// against this one, unrelated to which targets elephc actually publishes
/// for (see `Upstream::start_for_target` and `start_with_missing_platform`
/// for fixtures that shape assets for a different target on purpose).
fn elvm_target() -> &'static str {
    "aarch64-apple-darwin"
}

fn elvm(sandbox: &Sandbox, upstream: &Upstream) -> Command {
    let mut cmd = Command::cargo_bin("elvm").unwrap();
    cmd.env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .env("ELVM_GITHUB_API", upstream.server.base_url())
        .env("ELVM_TARGET", elvm_target())
        .env_remove("ELEPHC_VERSION")
        .current_dir(sandbox.home());
    cmd
}

#[test]
fn installs_a_release_with_its_bridge_archives_flat() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start("0.26.4");

    elvm(&sandbox, &upstream)
        .args(["install", "0.26.4"])
        .assert()
        .success();

    let dir = sandbox.elvm_dir().join("versions/0.26.4");
    let binary = dir.join("elephc");
    assert!(binary.is_file());

    // Pin that the installed binary is actually executable: a regression
    // in `archive::extract_tar_gz` that stops applying entry permissions
    // (e.g. a rewrite that copies entry bytes by hand instead of calling
    // `unpack`) would otherwise still pass every check above while leaving
    // the shim to fail at exec time with EACCES. Verified this fires: with
    // extraction rewritten to manually copy bytes via `io::copy` instead
    // of calling `tar::Entry::unpack`, this assertion fails (mode 100644).
    // Note `set_preserve_permissions` itself does NOT gate this — per the
    // `tar` crate's source, it only controls preservation of bits above
    // 0o777 (setuid/setgid/sticky); the base rwx bits from the header are
    // always applied. What actually protects executability is `unpack`
    // being called at all, and the fixture below setting mode 0o755.
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&binary).unwrap().permissions().mode();
    assert!(
        mode & 0o111 != 0,
        "installed elephc is not executable: mode {mode:o}"
    );

    for archive in support::BRIDGE_ARCHIVES {
        assert!(
            dir.join(archive).is_file(),
            "missing {archive} beside the binary"
        );
    }
}

#[test]
fn installing_latest_picks_the_highest_published_version() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_many(&["0.25.2", "0.26.4"], None);
    // A *higher* version is already installed locally. `install latest`
    // must still resolve to the highest *published* version, not the
    // highest installed one — this is the spec's deliberate asymmetry
    // with resolution's `latest`, which means "highest installed".
    sandbox.fake_elephc("0.27.0");

    elvm(&sandbox, &upstream)
        .args(["install", "latest"])
        .assert()
        .success();

    assert!(sandbox.elvm_dir().join("versions/0.26.4/elephc").is_file());
    assert!(!sandbox.elvm_dir().join("versions/0.25.2").exists());
}

#[test]
fn a_corrupt_download_leaves_nothing_installed() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_corrupt("0.26.4");

    elvm(&sandbox, &upstream)
        .args(["install", "0.26.4", "--force"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("checksum"));

    assert!(!sandbox.elvm_dir().join("versions/0.26.4").exists());
}

#[test]
fn a_failed_force_reinstall_leaves_the_working_install_in_place() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_corrupt("0.26.4");
    sandbox.fake_elephc("0.26.4");

    elvm(&sandbox, &upstream)
        .args(["install", "0.26.4", "--force"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("checksum"));

    // The replacement never verified, so the working install that was
    // there before `--force` was passed must still be there after.
    assert!(sandbox.elvm_dir().join("versions/0.26.4/elephc").is_file());
}

#[test]
fn reinstalling_without_force_is_refused() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start("0.26.4");
    sandbox.fake_elephc("0.26.4");

    elvm(&sandbox, &upstream)
        .args(["install", "0.26.4"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("already installed"))
        .stderr(predicates::str::contains("--force"));
}

/// Whether a binary exists for a host is a property of the release data, not
/// a fixed platform list: a release that actually carries the requested
/// host's asset installs cleanly on a non-macOS target. This is the
/// regression guard for the bug itself — under the old hardcoded
/// `PUBLISHED_TARGETS` constant, this failed even though the binary existed.
#[test]
fn installing_a_release_that_carries_the_hosts_asset_succeeds_on_a_non_macos_target() {
    let sandbox = Sandbox::new();
    let target = "x86_64-unknown-linux-gnu";
    let upstream = Upstream::start_for_target("0.26.4", target);

    let mut cmd = elvm(&sandbox, &upstream);
    cmd.env("ELVM_TARGET", target)
        .args(["install", "0.26.4"])
        .assert()
        .success();

    assert!(sandbox.elvm_dir().join("versions/0.26.4/elephc").is_file());
}

/// When the *requested* release has no binary for the host but a *later*
/// release does, the error names that later release instead of pointing at
/// `--build` — `--build` is only right when nothing published works here at
/// all. `start_with_missing_platform("0.24.3", "0.25.2")` builds exactly
/// this shape for a Linux host: "0.24.3" (the `available` param) gets only
/// the fixture's default `elvm_target()` asset (macOS), and "0.25.2" (the
/// `unavailable` param) gets its hardcoded `x86_64-unknown-linux-gnu` asset
/// — so requesting "0.24.3" under a Linux host has no binary, while "0.25.2"
/// does and is the only (hence earliest) release that does.
#[test]
fn requesting_a_version_with_no_host_binary_names_the_earliest_release_that_has_one() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_with_missing_platform("0.24.3", "0.25.2");

    let mut cmd = elvm(&sandbox, &upstream);
    let assert = cmd
        .env("ELVM_TARGET", "x86_64-unknown-linux-gnu")
        .args(["install", "0.24.3"])
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();

    assert!(stderr.contains("no published binary"), "{stderr}");
    assert!(stderr.contains("0.25.2"), "{stderr}");
    assert!(stderr.contains("elvm install 0.25.2"), "{stderr}");
    // A release that works for this host exists, so this must not fall back
    // to suggesting a from-source build.
    assert!(!stderr.contains("--build"), "{stderr}");
}

/// When *no* published release carries a binary for the host at all, the
/// error falls back to `--build` — the only case where that is actually the
/// fix.
#[test]
fn requesting_a_version_falls_back_to_build_when_no_release_has_a_host_binary() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start("0.26.4");

    let mut cmd = elvm(&sandbox, &upstream);
    cmd.env("ELVM_TARGET", "x86_64-unknown-linux-gnu")
        .args(["install", "0.26.4"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no published binary"))
        .stderr(predicates::str::contains("--build"));
}

#[test]
fn install_with_no_argument_reads_the_version_file() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start("0.26.4");
    let project = sandbox.home().join("project");
    sandbox.write_version_file(&project, "0.26.4\n");

    let mut cmd = elvm(&sandbox, &upstream);
    cmd.current_dir(&project).arg("install").assert().success();

    assert!(sandbox.elvm_dir().join("versions/0.26.4/elephc").is_file());
}

#[test]
fn ls_remote_lists_published_versions_and_marks_installed_ones() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_many(&["0.25.2", "0.26.4"], None);
    // Install only one of the two published versions
    sandbox.fake_elephc("0.26.4");

    elvm(&sandbox, &upstream)
        .arg("ls-remote")
        .assert()
        .success()
        .stdout(predicates::str::contains("* 0.26.4"))
        .stdout(predicates::str::contains("  0.25.2"));
}

#[test]
fn ls_remote_marks_releases_with_no_binary_for_this_platform() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_with_missing_platform("0.26.4", "0.25.2");

    elvm(&sandbox, &upstream)
        .arg("ls-remote")
        .assert()
        .success()
        .stdout(predicates::str::contains("  0.26.4"))
        .stdout(predicates::str::contains(
            "  0.25.2  (no binary for this platform)",
        ));
}

/// The default (grouped) shape: a summary header naming both counts, the
/// newest (major, minor) series listed patch by patch with the highest
/// version last, and every older series collapsed to its highest patch with
/// a `(+N)` count of what's beneath it — oldest series first, so the very
/// last line on screen is always the newest release.
#[test]
fn ls_remote_default_output_groups_older_series_and_expands_the_newest() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_many(
        &["0.24.0", "0.25.0", "0.25.1", "0.25.2", "0.26.0", "0.26.1"],
        None,
    );

    let assert = elvm(&sandbox, &upstream)
        .arg("ls-remote")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    // Header names both counts and the flag to see everything; with only 3
    // series and none hidden, there's no "N older series hidden" line.
    assert!(
        stdout.contains("6 releases in 3 series — elvm ls-remote --all for every patch"),
        "{stdout}"
    );
    assert!(!stdout.contains("hidden"), "{stdout}");

    // A series with only one patch collapses with no (+N) suffix at all.
    assert!(stdout.contains("0.24.0"), "{stdout}");
    assert!(!stdout.contains("0.24.0  (+"), "{stdout}");

    // Older series collapse to their highest patch with a count of the rest;
    // the patches beneath it must not get their own line.
    assert!(stdout.contains("0.25.2  (+2)"), "{stdout}");
    assert!(!stdout.contains("0.25.0"), "{stdout}");
    assert!(!stdout.contains("0.25.1"), "{stdout}");

    // Newest series (0.26) is expanded patch by patch, with no (+N) suffix,
    // and only its last (highest) line carries the "latest" marker.
    assert!(stdout.contains("0.26.0"), "{stdout}");
    assert!(!stdout.contains("0.26.0  (+"), "{stdout}");
    assert!(stdout.contains("0.26.1  ← latest"), "{stdout}");

    // Oldest-first, newest-last: the collapsed block precedes the expanded
    // one, and within it "0.26.1  ← latest" is the very last line printed.
    let pos = |needle: &str| stdout.find(needle).unwrap();
    assert!(pos("0.24.0") < pos("0.25.2"));
    assert!(pos("0.25.2") < pos("0.26.0"));
    assert!(pos("0.26.0") < pos("0.26.1  ← latest"));
    assert!(stdout.trim_end().ends_with("0.26.1  ← latest"));
}

/// Review finding on `main` (post `5f2d9c7`): when the published list spans
/// exactly one `(major, minor)` series, there is no collapsed block, so row
/// 0 of `group()`'s output is itself `expanded`. The header block always
/// prints one blank line as a separator, and the row loop separately prints
/// one the first time it sees an `expanded` row — with no collapsed block
/// to separate from, that's the same seam twice, producing two blank lines
/// back to back instead of one. Pins the fix by asserting the exact
/// expected bytes, not just "some blank line exists somewhere".
#[test]
fn ls_remote_prints_exactly_one_blank_line_when_everything_is_one_series() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_many(&["0.26.0", "0.26.1", "0.26.2"], None);

    let assert = elvm(&sandbox, &upstream)
        .arg("ls-remote")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    let expected = "3 releases in 1 series — elvm ls-remote --all for every patch\n\n  0.26.0\n  0.26.1\n  0.26.2  ← latest\n";
    assert_eq!(stdout, expected, "{stdout:?}");
}

/// `--all` lists every published patch, flat: no collapsing, no "latest"
/// marker, no summary header, no hidden-series line — just every release,
/// oldest first, newest last, with the usual `*`.
#[test]
fn ls_remote_all_lists_every_published_patch_with_no_grouping() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start_many(&["0.24.0", "0.25.0", "0.25.1", "0.26.0"], None);

    let assert = elvm(&sandbox, &upstream)
        .args(["ls-remote", "--all"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    for version in ["0.24.0", "0.25.0", "0.25.1", "0.26.0"] {
        assert!(stdout.contains(version), "missing {version} in:\n{stdout}");
    }
    assert!(!stdout.contains("(+"), "{stdout}");
    assert!(!stdout.contains("← latest"), "{stdout}");
    assert!(!stdout.contains("releases in"), "{stdout}");
    assert!(!stdout.contains("hidden"), "{stdout}");

    // Oldest first, newest last.
    let pos = |needle: &str| stdout.find(needle).unwrap();
    assert!(pos("0.24.0") < pos("0.25.0"));
    assert!(pos("0.25.0") < pos("0.25.1"));
    assert!(pos("0.25.1") < pos("0.26.0"));
    assert!(stdout.trim_end().ends_with("0.26.0"));
}

/// Guards the pagination bug directly: GitHub returns releases 100 to a
/// page, and `github::list_releases` used to request only page 1, silently
/// dropping everything older. Page 1 here is a full 100-entry page (which is
/// what makes a correct client request page 2 at all); page 2 carries an old
/// version that only pagination can surface. Verified this fails without the
/// pagination loop — see the report for the actual output of that run.
#[test]
fn ls_remote_all_lists_a_version_that_only_exists_on_the_second_page() {
    let sandbox = Sandbox::new();
    let server = MockServer::start();

    let page1_entries: Vec<String> = (0..100)
        .map(|i| format!(r#"{{"tag_name":"v0.20.{i}","assets":[]}}"#))
        .collect();
    let page1 = format!("[{}]", page1_entries.join(","));
    let page2 = r#"[
        {"tag_name":"v0.16.0","assets":[]},
        {"tag_name":"v0.16.1","assets":[]}
    ]"#;

    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/illegalstudio/elephc/releases")
            .query_param("page", "1");
        then.status(200)
            .header("content-type", "application/json")
            .body(page1);
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/illegalstudio/elephc/releases")
            .query_param("page", "2");
        then.status(200)
            .header("content-type", "application/json")
            .body(page2);
    });

    let sandbox_home = sandbox.home();
    let assert = Command::cargo_bin("elvm")
        .unwrap()
        .env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", &sandbox_home)
        .env("ELVM_GITHUB_API", server.base_url())
        .env("ELVM_TARGET", elvm_target())
        .env_remove("ELEPHC_VERSION")
        .current_dir(&sandbox_home)
        .args(["ls-remote", "--all"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(
        stdout.contains("0.16.0") && stdout.contains("0.16.1"),
        "expected page-2-only versions 0.16.0/0.16.1 in output, pagination is broken:\n{stdout}"
    );
}

/// Review finding on 645aec7: the pagination test above proves a *fresh*
/// fetch merges every page, but nothing pinned that a *cache-served* call
/// (i.e. `github::list_releases` reading `cache/releases.json` back off
/// disk within its 600s TTL) still holds every page merged, rather than
/// just whichever page happened to be fetched last. `ls-remote` can't stand
/// in for this: `ls_remote::run` calls `list_releases(paths, true)`, which
/// skips the cache-read branch and always re-fetches. `install` is the
/// caller that passes `refresh: false` and genuinely reads the on-disk
/// cache when it's fresh, so that's what this drives instead.
///
/// Page 1 is a full 100-entry page (0.20.0..0.20.99, so a correct client
/// requests page 2 at all); page 2 carries two releases, 0.16.0 and 0.16.1,
/// the latter the only place its download assets exist. A cold install of
/// 0.16.1 forces the full paginated fetch and the cache write. The `page=2`
/// route is then deleted outright, so a fresh fetch could not possibly see
/// either page-2 release again. A second install, of a *page-1* release
/// (0.20.50), only succeeds if the on-disk cache still holds page 1's 100
/// entries merged with page 2's — a "cache only the last page" regression
/// would leave the cache holding just 0.16.0/0.16.1, and this install would
/// fail with "not a published release". Re-installing 0.16.1 with `--force`
/// (page 2's own release, off the same cache) closes the loop.
///
/// Confirmed this discriminates the regression it's named for: with the
/// cache write in `github::list_releases` changed from writing the merged
/// `entries` array to writing the *last page's own response body*, this
/// test fails — see the report for the actual output of that run.
#[test]
fn cache_served_release_list_still_holds_every_paginated_page() {
    let sandbox = Sandbox::new();
    let server = MockServer::start();
    let tarball = fake_release_tarball();
    let digest = sha256_hex(&tarball);

    const PAGE1_MARKER: &str = "0.20.50";
    const PAGE2_MARKER: &str = "0.16.1";

    let asset_urls = |version: &str| -> (String, String, String, String) {
        let tar_name = format!("elephc-v{version}-{}.tar.gz", elvm_target());
        let tar_url = server.url(format!("/download/{tar_name}"));
        let sha_url = server.url(format!("/download/{tar_name}.sha256"));
        (
            tar_name.clone(),
            tar_url,
            format!("{tar_name}.sha256"),
            sha_url,
        )
    };

    let (page1_tar_name, page1_tar_url, page1_sha_name, page1_sha_url) = asset_urls(PAGE1_MARKER);
    let (page2_tar_name, page2_tar_url, page2_sha_name, page2_sha_url) = asset_urls(PAGE2_MARKER);

    for (tar_name, sha_name) in [
        (&page1_tar_name, &page1_sha_name),
        (&page2_tar_name, &page2_sha_name),
    ] {
        let tarball = tarball.clone();
        let digest = digest.clone();
        server.mock(|when, then| {
            when.method(GET).path(format!("/download/{tar_name}"));
            then.status(200).body(tarball.clone());
        });
        server.mock(|when, then| {
            when.method(GET).path(format!("/download/{sha_name}"));
            then.status(200).body(format!("{digest}  {tar_name}\n"));
        });
    }

    // Page 1: a full 100-entry page. Only 0.20.50 (PAGE1_MARKER) gets
    // download assets, since it's the only page-1 release this test
    // actually installs.
    let page1_entries: Vec<String> = (0..100)
        .map(|i| {
            let version = format!("0.20.{i}");
            if version == PAGE1_MARKER {
                format!(
                    r#"{{"tag_name":"v{version}","assets":[
                        {{"name":"{page1_tar_name}","browser_download_url":"{page1_tar_url}"}},
                        {{"name":"{page1_sha_name}","browser_download_url":"{page1_sha_url}"}}
                    ]}}"#
                )
            } else {
                format!(r#"{{"tag_name":"v{version}","assets":[]}}"#)
            }
        })
        .collect();
    let page1 = format!("[{}]", page1_entries.join(","));

    // Page 2: two releases; 0.16.1 (PAGE2_MARKER) is the distinctive one
    // whose assets only ever appear here.
    let page2 = format!(
        r#"[
            {{"tag_name":"v0.16.0","assets":[]}},
            {{"tag_name":"v{PAGE2_MARKER}","assets":[
                {{"name":"{page2_tar_name}","browser_download_url":"{page2_tar_url}"}},
                {{"name":"{page2_sha_name}","browser_download_url":"{page2_sha_url}"}}
            ]}}
        ]"#
    );

    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/illegalstudio/elephc/releases")
            .query_param("page", "1");
        then.status(200)
            .header("content-type", "application/json")
            .body(page1);
    });
    let mut page2_route = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/illegalstudio/elephc/releases")
            .query_param("page", "2");
        then.status(200)
            .header("content-type", "application/json")
            .body(page2);
    });

    let sandbox_home = sandbox.home();
    let elvm_cmd = || {
        let mut cmd = Command::cargo_bin("elvm").unwrap();
        cmd.env("ELVM_DIR", sandbox.elvm_dir())
            .env("HOME", &sandbox_home)
            .env("ELVM_GITHUB_API", server.base_url())
            .env("ELVM_TARGET", elvm_target())
            .env_remove("ELEPHC_VERSION")
            .current_dir(&sandbox_home);
        cmd
    };

    // Cold cache: installing the page-2-only release forces the full
    // paginated fetch (page 1 + page 2) and writes the merged list to
    // cache/releases.json.
    elvm_cmd()
        .args(["install", PAGE2_MARKER])
        .assert()
        .success();
    assert!(sandbox
        .elvm_dir()
        .join(format!("versions/{PAGE2_MARKER}/elephc"))
        .is_file());

    // Page 2 is now unreachable: a fresh fetch could not possibly see
    // either of its releases again.
    page2_route.delete();

    // Within the 600s TTL, install a *page-1* release. This only succeeds
    // if the on-disk cache still holds page 1's 100 entries merged with
    // page 2's, not just whichever page was written last.
    elvm_cmd()
        .args(["install", PAGE1_MARKER])
        .assert()
        .success();
    assert!(sandbox
        .elvm_dir()
        .join(format!("versions/{PAGE1_MARKER}/elephc"))
        .is_file());

    // And the page-2-only release itself must still resolve off the same
    // cache.
    elvm_cmd()
        .args(["install", PAGE2_MARKER, "--force"])
        .assert()
        .success();
}

/// IMPORTANT 3 / spec §4.5 step 3: a tarball already verified in
/// `cache/downloads/` must be reused rather than re-downloaded. Installs
/// once for real, then reinstalls with `--force` against a *second* upstream
/// whose tarball route always fails — the checksum route still serves the
/// digest that matches what is already cached, so the only way this can
/// succeed is by never hitting the tarball route at all. `assert_hits(0)`
/// pins that directly, rather than relying on failure-by-side-effect.
#[test]
fn a_verified_cached_tarball_is_reused_instead_of_re_downloaded() {
    let sandbox = Sandbox::new();
    let first = Upstream::start("0.26.4");

    elvm(&sandbox, &first)
        .args(["install", "0.26.4"])
        .assert()
        .success();

    let tar_name = format!("elephc-v0.26.4-{}.tar.gz", elvm_target());
    let cached = sandbox.elvm_dir().join("cache/downloads").join(&tar_name);
    assert!(
        cached.is_file(),
        "the verified tarball must be left in cache/downloads/"
    );
    let digest = sha256_hex(&std::fs::read(&cached).unwrap());

    // Take the first server down and drop the cached release list it served:
    // otherwise `list_releases` keeps answering from its 600s cache and the
    // asset URLs it hands back still point at server #1, which would happily
    // serve them. That would make the assertion below trivially true whether
    // or not cache-reuse is implemented at all.
    drop(first);
    std::fs::remove_file(sandbox.elvm_dir().join("cache/releases.json")).unwrap();

    // A fresh server: same digest (matching what's already cached), but the
    // tarball body route fails outright if it is ever requested.
    let server = MockServer::start();
    let tar_url = server.url(format!("/download/{tar_name}"));
    let sha_url = server.url(format!("/download/{tar_name}.sha256"));
    let tarball_route = server.mock(|when, then| {
        when.method(GET).path(format!("/download/{tar_name}"));
        then.status(500);
    });
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/download/{tar_name}.sha256"));
        then.status(200).body(format!("{digest}  {tar_name}\n"));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/illegalstudio/elephc/releases");
        then.status(200)
            .header("content-type", "application/json")
            .body(format!(
                r#"[{{"tag_name":"v0.26.4","assets":[
                    {{"name":"{tar_name}","browser_download_url":"{tar_url}"}},
                    {{"name":"{tar_name}.sha256","browser_download_url":"{sha_url}"}}
                ]}}]"#,
            ));
    });

    Command::cargo_bin("elvm")
        .unwrap()
        .env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .env("ELVM_GITHUB_API", server.base_url())
        .env("ELVM_TARGET", elvm_target())
        .env_remove("ELEPHC_VERSION")
        .current_dir(sandbox.home())
        .args(["install", "0.26.4", "--force"])
        .assert()
        .success();

    tarball_route.assert_hits(0);
}

#[test]
fn a_successful_force_reinstall_replaces_the_working_install() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start("0.26.4");
    // Pre-install a fake version; note its distinctive output differs from
    // the fixture tarball's "fake-elephc $*" (no version name in output).
    sandbox.fake_elephc("0.26.4");

    // Verify the fake is in place before --force
    let installed = sandbox.elvm_dir().join("versions/0.26.4/elephc");
    let fake_content = std::fs::read_to_string(&installed).unwrap();
    assert!(fake_content.contains("fake-elephc 0.26.4 $*"));

    // Force-reinstall from the fixture tarball
    elvm(&sandbox, &upstream)
        .args(["install", "0.26.4", "--force"])
        .assert()
        .success();

    // The binary should now be from the tarball, not the fake. The tarball
    // has "fake-elephc $*" with no version name, so this proves the
    // directory was actually replaced, not left alone.
    let replaced_content = std::fs::read_to_string(&installed).unwrap();
    assert!(
        replaced_content.contains("fake-elephc $*"),
        "binary should be from tarball: {replaced_content}"
    );
    assert!(!replaced_content.contains("fake-elephc 0.26.4"));
}
