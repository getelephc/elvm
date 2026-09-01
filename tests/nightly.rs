//! The nightly channel, end to end against a mock of elephc's release API.
//!
//! What these pin, beyond "it installs": that a nightly never leaks into
//! release resolution, that reinstalling the rolling channel is an update
//! rather than an error, and that a build is identified by the digest of its
//! tarball rather than by a version string two builds can share.

mod support;

use assert_cmd::Command;
use httpmock::prelude::*;
use predicates::prelude::PredicateBooleanExt;
use support::Sandbox;

const TARGET: &str = "aarch64-apple-darwin";

/// A tarball with the shape elephc's nightly workflow packs: `elephc` plus
/// the bridge archives, flat, no top-level directory. `marker` changes the
/// bytes so two builds differ in digest, as two nightlies do.
fn fake_nightly_tarball(marker: &str) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));

    let script = format!("#!/bin/sh\necho \"fake-elephc {marker} $*\"\n");
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, "elephc", script.as_bytes())
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

/// One published nightly, as the workflow publishes it.
struct Build {
    tag: &'static str,
    version: &'static str,
    commit: &'static str,
    published_at: &'static str,
    /// Distinguishes this build's bytes from another's.
    marker: &'static str,
}

struct Upstream {
    server: MockServer,
}

impl Upstream {
    /// Serves the nightly channel: every build under its dated tag, and the
    /// first one also under the rolling `nightly` tag — which is exactly
    /// what upstream does, publishing the same artifacts twice so the
    /// `/download/nightly/` URLs stay valid.
    fn start(builds: &[Build], releases: &str) -> Self {
        let server = MockServer::start();

        for (index, build) in builds.iter().enumerate() {
            let tarball = fake_nightly_tarball(build.marker);
            let tar_name = format!("elephc-nightly-{TARGET}.tar.gz");
            // The asset name carries no version, so each tag needs its own
            // download path — the same split upstream gets from the tag in
            // the URL.
            let tar_path = format!("/download/{}/{tar_name}", build.tag);
            let sha_path = format!("{tar_path}.sha256");

            server.mock(|when, then| {
                when.method(GET).path(tar_path.clone());
                then.status(200).body(tarball.clone());
            });
            server.mock(|when, then| {
                when.method(GET).path(sha_path.clone());
                then.status(200)
                    .body(format!("{}  {tar_name}\n", sha256_hex(&tarball)));
            });

            let body = format!(
                r#"{{"tag_name":"{}","name":"Nightly {}","target_commitish":"{}",
                     "published_at":"{}","created_at":"2000-01-01T00:00:00Z",
                     "prerelease":true,"assets":[
                       {{"name":"{tar_name}","browser_download_url":"{}"}},
                       {{"name":"{tar_name}.sha256","browser_download_url":"{}"}}
                     ]}}"#,
                build.tag,
                build.version,
                build.commit,
                build.published_at,
                server.url(&tar_path),
                server.url(&sha_path),
            );

            server.mock(|when, then| {
                when.method(GET).path(format!(
                    "/repos/illegalstudio/elephc/releases/tags/{}",
                    build.tag
                ));
                then.status(200)
                    .header("content-type", "application/json")
                    .body(body.clone());
            });

            // The rolling tag serves the newest build's artifacts.
            if index == 0 {
                let rolling = body.replace(
                    &format!(r#""tag_name":"{}""#, build.tag),
                    r#""tag_name":"nightly""#,
                );
                server.mock(|when, then| {
                    when.method(GET)
                        .path("/repos/illegalstudio/elephc/releases/tags/nightly");
                    then.status(200)
                        .header("content-type", "application/json")
                        .body(rolling);
                });
            }
        }

        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/illegalstudio/elephc/releases");
            then.status(200)
                .header("content-type", "application/json")
                .body(releases);
        });

        // A tag with no mock gets httpmock's own 404 — which is exactly how
        // a pruned dated tag and an empty channel look from the API.

        Self { server }
    }
}

fn build(
    tag: &'static str,
    version: &'static str,
    published_at: &'static str,
    marker: &'static str,
) -> Build {
    Build {
        tag,
        version,
        commit: "672ff38c5aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        published_at,
        marker,
    }
}

/// The release list upstream serves alongside the nightlies: one stable
/// release, plus the nightly prereleases that must never be mistaken for it.
///
/// Each dated entry gets a `published_at` derived from its own tag, because
/// that timestamp — not the tag name — is what orders the listing.
fn releases_with(nightly_tags: &[&str]) -> String {
    let mut entries = vec![r#"{"tag_name":"v0.26.5","assets":[]}"#.to_string()];
    for tag in nightly_tags {
        let date = tag.strip_prefix("nightly-").unwrap_or("20260101");
        let (y, md) = date.split_at(4);
        let (m, rest) = md.split_at(2);
        let d = &rest[..2];
        entries.push(format!(
            r#"{{"tag_name":"{tag}","name":"Nightly 0.26.5-nightly.{date}+gaaa",
                 "published_at":"{y}-{m}-{d}T03:40:00Z","prerelease":true,"assets":[]}}"#
        ));
    }
    format!("[{}]", entries.join(","))
}

fn elvm(sandbox: &Sandbox, upstream: &Upstream) -> Command {
    let mut cmd = Command::cargo_bin("elvm").unwrap();
    cmd.env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .env("ELVM_GITHUB_API", upstream.server.base_url())
        .env("ELVM_TARGET", TARGET)
        .env_remove("ELEPHC_VERSION")
        .current_dir(sandbox.home());
    cmd
}

#[test]
fn installs_the_rolling_channel_under_its_own_directory() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(
        &[build(
            "nightly-20260901",
            "0.26.5-nightly.20260901+g672ff38c5",
            "2026-09-01T15:20:00Z",
            "a",
        )],
        &releases_with(&["nightly", "nightly-20260901"]),
    );

    elvm(&sandbox, &upstream)
        .args(["install", "nightly"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "installed elephc 0.26.5-nightly.20260901+g672ff38c5 (published 2026-09-01) as nightly",
        ));

    let dir = sandbox.elvm_dir().join("versions/nightly");
    assert!(dir.join("elephc").is_file(), "binary missing in {dir:?}");
    for archive in support::BRIDGE_ARCHIVES {
        assert!(dir.join(archive).is_file(), "{archive} missing");
    }

    // The stamp is what lets elvm tell one nightly from another later: the
    // directory name is `nightly` every single night.
    let stamp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".elvm-nightly.json")).unwrap())
            .unwrap();
    assert_eq!(stamp["version"], "0.26.5-nightly.20260901+g672ff38c5");
    assert_eq!(stamp["tag"], "nightly");
    assert_eq!(stamp["sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn a_nightly_is_never_selected_by_latest_or_by_a_version_prefix() {
    // The property the whole design rests on. `0.26.5-nightly.20260901`
    // is valid semver and would sort above an installed 0.26.4, so if a
    // nightly ever entered the version list, `latest` in a `.elephc-version`
    // would silently start resolving to an unsupported build of main.
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(
        &[build(
            "nightly-20260901",
            "0.26.5-nightly.20260901+g672ff38c5",
            "2026-09-01T15:20:00Z",
            "a",
        )],
        &releases_with(&["nightly", "nightly-20260901"]),
    );

    sandbox.fake_elephc("0.26.4");
    elvm(&sandbox, &upstream)
        .args(["install", "nightly"])
        .assert()
        .success();

    for request in ["latest", "0.26", "0"] {
        let assert = elvm(&sandbox, &upstream)
            .args(["which", request])
            .assert()
            .success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert!(
            stdout.contains("versions/0.26.4/elephc"),
            "{request} resolved to {stdout}"
        );
    }

    // And the reverse direction: `nightly` resolves to the nightly, not to
    // the highest installed release.
    let assert = elvm(&sandbox, &upstream)
        .args(["which", "nightly"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("versions/nightly/elephc"), "{stdout}");
}

#[test]
fn reinstalling_the_same_build_is_a_no_op_not_an_error() {
    // Unlike a release, where a second `install` is an error that `--force`
    // overrides: `nightly` is a channel, so installing it again is "update
    // me", and being already current is success.
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(
        &[build(
            "nightly-20260901",
            "0.26.5-nightly.20260901+g672ff38c5",
            "2026-09-01T15:20:00Z",
            "a",
        )],
        &releases_with(&["nightly", "nightly-20260901"]),
    );

    elvm(&sandbox, &upstream)
        .args(["install", "nightly"])
        .assert()
        .success();
    elvm(&sandbox, &upstream)
        .args(["install", "nightly"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "elephc nightly is already up to date: 0.26.5-nightly.20260901+g672ff38c5",
        ))
        .stdout(predicates::str::contains("downloading").not());
}

#[test]
fn a_newer_build_replaces_the_installed_one_without_force() {
    let sandbox = Sandbox::new();
    let first = Upstream::start(
        &[build(
            "nightly-20260901",
            "0.26.5-nightly.20260901+gaaaaaaaaa",
            "2026-09-01T15:20:00Z",
            "first",
        )],
        &releases_with(&["nightly", "nightly-20260901"]),
    );
    elvm(&sandbox, &first)
        .args(["install", "nightly"])
        .assert()
        .success();

    // A second night: same channel, different bytes. The version string
    // alone would not prove the swap happened — two builds from one UTC day
    // share everything but the `+g<sha>` metadata — so the installed binary
    // is what gets checked.
    let second = Upstream::start(
        &[build(
            "nightly-20260902",
            "0.26.5-nightly.20260902+gbbbbbbbbb",
            "2026-09-02T03:41:00Z",
            "second",
        )],
        &releases_with(&["nightly", "nightly-20260902"]),
    );
    elvm(&sandbox, &second)
        .args(["install", "nightly"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "downloading elephc 0.26.5-nightly.20260902",
        ));

    let binary =
        std::fs::read_to_string(sandbox.elvm_dir().join("versions/nightly/elephc")).unwrap();
    assert!(binary.contains("second"), "stale binary: {binary}");
}

#[test]
fn a_dated_tag_installs_under_that_tag_and_coexists_with_the_rolling_one() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(
        &[
            build(
                "nightly-20260902",
                "0.26.5-nightly.20260902+gbbbbbbbbb",
                "2026-09-02T03:41:00Z",
                "second",
            ),
            build(
                "nightly-20260901",
                "0.26.5-nightly.20260901+gaaaaaaaaa",
                "2026-09-01T15:20:00Z",
                "first",
            ),
        ],
        &releases_with(&["nightly", "nightly-20260902", "nightly-20260901"]),
    );

    elvm(&sandbox, &upstream)
        .args(["install", "nightly"])
        .assert()
        .success();
    elvm(&sandbox, &upstream)
        .args(["install", "nightly-20260901"])
        .assert()
        .success();

    // The pin keeps its own directory: updating the channel later cannot
    // take it away, which is the entire point of pinning one.
    let pinned =
        std::fs::read_to_string(sandbox.elvm_dir().join("versions/nightly-20260901/elephc"))
            .unwrap();
    assert!(pinned.contains("first"), "{pinned}");
    let rolling =
        std::fs::read_to_string(sandbox.elvm_dir().join("versions/nightly/elephc")).unwrap();
    assert!(rolling.contains("second"), "{rolling}");
}

#[test]
fn a_pruned_dated_tag_names_what_is_still_published() {
    // Upstream keeps a limited window of dated nightlies and deletes the
    // rest, tag included — so this is the error a pin hits once it ages out,
    // and it has to say more than "not found".
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(
        &[build(
            "nightly-20260902",
            "0.26.5-nightly.20260902+gbbbbbbbbb",
            "2026-09-02T03:41:00Z",
            "second",
        )],
        &releases_with(&["nightly", "nightly-20260902"]),
    );

    elvm(&sandbox, &upstream)
        .args(["install", "nightly-20260801"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "elephc nightly-20260801 is not published",
        ))
        .stderr(predicates::str::contains("kept for a limited window"))
        .stderr(predicates::str::contains("nightly-20260902"));
}

#[test]
fn an_empty_channel_is_reported_as_such_not_as_a_missing_release() {
    // The workflow skips a night when main is red or unchanged, so "no
    // nightly right now" is a normal state upstream, not a broken install.
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(&[], &releases_with(&[]));

    elvm(&sandbox, &upstream)
        .args(["install", "nightly"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "the nightly channel has no published build",
        ));
}

#[test]
fn a_version_file_can_pin_a_nightly_and_install_reads_it() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(
        &[build(
            "nightly-20260901",
            "0.26.5-nightly.20260901+g672ff38c5",
            "2026-09-01T15:20:00Z",
            "a",
        )],
        &releases_with(&["nightly", "nightly-20260901"]),
    );

    let project = sandbox.home().join("project");
    sandbox.write_version_file(&project, "nightly-20260901\n");

    elvm(&sandbox, &upstream)
        .current_dir(&project)
        .arg("install")
        .assert()
        .success();

    // And the shim resolves it with no network access at all.
    let assert = elvm(&sandbox, &upstream)
        .current_dir(&project)
        .arg("current")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("nightly-20260901"), "{stdout}");
}

#[test]
fn use_warns_that_the_rolling_channel_does_not_pin_a_compiler() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(
        &[build(
            "nightly-20260901",
            "0.26.5-nightly.20260901+g672ff38c5",
            "2026-09-01T15:20:00Z",
            "a",
        )],
        &releases_with(&["nightly", "nightly-20260901"]),
    );

    elvm(&sandbox, &upstream)
        .args(["install", "nightly"])
        .assert()
        .success();
    elvm(&sandbox, &upstream)
        .args(["install", "nightly-20260901"])
        .assert()
        .success();

    elvm(&sandbox, &upstream)
        .args(["use", "nightly"])
        .assert()
        .success()
        .stderr(predicates::str::contains("does not pin a compiler"));

    // A dated tag *is* a pin, so it must not carry the warning.
    elvm(&sandbox, &upstream)
        .args(["use", "nightly-20260901"])
        .assert()
        .success()
        .stderr(predicates::str::contains("does not pin a compiler").not());
}

#[test]
fn ls_remote_lists_dated_nightlies_without_displacing_the_newest_release() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(
        &[],
        &releases_with(&["nightly", "nightly-20260902", "nightly-20260901"]),
    );

    let assert = elvm(&sandbox, &upstream)
        .arg("ls-remote")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("nightly-20260901"), "{stdout}");
    assert!(stdout.contains("nightly-20260902"), "{stdout}");
    // The rolling tag duplicates the newest dated build's artifacts; listing
    // it too would show the same build twice.
    assert!(
        !stdout.lines().any(|line| line.starts_with("  nightly  ")),
        "{stdout}"
    );
    // The nightly block sits above the releases: the last line of ls-remote
    // stays the newest published release.
    assert!(stdout.trim_end().ends_with("0.26.5  ← latest"), "{stdout}");
}

#[test]
fn link_refuses_to_take_a_nightly_name() {
    // `versions/nightly` is where the channel installs; a link there would
    // be silently replaced by the next `elvm install nightly`.
    let sandbox = Sandbox::new();
    let upstream = Upstream::start(&[], &releases_with(&[]));
    let checkout = sandbox.home().join("checkout");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::write(checkout.join("elephc"), b"#!/bin/sh\n").unwrap();
    support::make_executable(&checkout.join("elephc"));

    for name in ["nightly", "nightly-20260901"] {
        elvm(&sandbox, &upstream)
            .args(["link", checkout.to_str().unwrap(), "--as", name])
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "reserved for the nightly channel",
            ));
    }
}
