mod support;

use assert_cmd::Command;
use httpmock::prelude::*;
use support::Sandbox;

/// Builds a tarball with the same flat shape as an elephc release.
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
        Self::start_with_checksum(version, None)
    }

    /// Like `start`, but the `.sha256` asset serves a digest that can never
    /// match the tarball, from the outset — so verification must fail before
    /// anything is extracted or installed.
    fn start_corrupt(version: &str) -> Self {
        Self::start_with_checksum(version, Some(format!("{}  x", "0".repeat(64))))
    }

    fn start_with_checksum(version: &str, bad_checksum: Option<String>) -> Self {
        let server = MockServer::start();
        let tarball = fake_release_tarball();
        let name = format!("elephc-v{version}-{}", elvm_target());
        let tar_name = format!("{name}.tar.gz");

        let tar_url = server.url(format!("/download/{tar_name}"));
        let sha_url = server.url(format!("/download/{tar_name}.sha256"));
        let releases = format!(
            r#"[{{"tag_name":"v{version}","assets":[
                {{"name":"{tar_name}","browser_download_url":"{tar_url}"}},
                {{"name":"{tar_name}.sha256","browser_download_url":"{sha_url}"}}
            ]}}]"#,
        );

        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/illegalstudio/elephc/releases");
            then.status(200)
                .header("content-type", "application/json")
                .body(releases);
        });
        server.mock(|when, then| {
            when.method(GET).path(format!("/download/{tar_name}"));
            then.status(200).body(tarball.clone());
        });

        let checksum_body = match bad_checksum {
            Some(digest) => format!("{digest}\n"),
            None => format!("{}  {tar_name}\n", sha256_hex(&tarball)),
        };
        server.mock(|when, then| {
            when.method(GET)
                .path(format!("/download/{tar_name}.sha256"));
            then.status(200).body(checksum_body.clone());
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
    assert!(dir.join("elephc").is_file());
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
    let upstream = Upstream::start("0.26.4");

    elvm(&sandbox, &upstream)
        .args(["install", "latest"])
        .assert()
        .success();

    assert!(sandbox.elvm_dir().join("versions/0.26.4/elephc").is_file());
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
fn install_with_no_argument_reads_the_version_file() {
    let sandbox = Sandbox::new();
    let upstream = Upstream::start("0.26.4");
    let project = sandbox.home().join("project");
    sandbox.write_version_file(&project, "0.26.4\n");

    let mut cmd = elvm(&sandbox, &upstream);
    cmd.current_dir(&project).arg("install").assert().success();

    assert!(sandbox.elvm_dir().join("versions/0.26.4/elephc").is_file());
}
