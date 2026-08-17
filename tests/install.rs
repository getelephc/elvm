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

    /// Serves only a 403 (rate-limited) response for the releases listing,
    /// and nothing else — no tarball or checksum routes exist at all. Used
    /// to prove the unpublished-target check runs before any network call:
    /// if it didn't, this would surface as a rate-limit error instead of
    /// the "no published binary" message.
    fn start_rate_limited() -> Self {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/illegalstudio/elephc/releases");
            then.status(403).header("x-ratelimit-reset", "0");
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

/// The one target elephc publishes; these tests exercise the download path,
/// which only exists for it.
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

#[test]
fn installing_on_an_unpublished_target_points_at_build() {
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
fn unpublished_target_is_rejected_before_any_network_call() {
    let sandbox = Sandbox::new();
    // No tarball or checksum route exists on this server at all, and the
    // releases listing itself is rate-limited. If the target check ran
    // after resolving the release list (as it used to), this would
    // surface as "GitHub rate limit exhausted" instead.
    let upstream = Upstream::start_rate_limited();

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
