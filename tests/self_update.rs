mod support;

use assert_cmd::Command;
use httpmock::prelude::*;
use support::Sandbox;

/// The one target elephc publishes; matches the other integration suites so
/// the asset name `self_update` builds (`elvm-v{version}-{target}.tar.gz`)
/// lines up with what the mock server serves.
const ELVM_TARGET: &str = "aarch64-apple-darwin";

/// Builds a tarball containing a single `elvm` entry, the shape a real
/// `elvm` release tarball has.
fn fake_elvm_tarball(content: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder.append_data(&mut header, "elvm", content).unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Serves `getelephc/elvm`'s "latest release" endpoint plus a tarball and
/// checksum for one version, in the shape `self_update::run` expects.
struct Upstream {
    server: MockServer,
}

impl Upstream {
    /// `bad_checksum`: when true, the `.sha256` asset serves a digest that
    /// can never match `tarball`, from the outset — httpmock resolves the
    /// first matching mock, so there is no way to swap in a bad checksum
    /// after the fact.
    fn start(version: &str, tarball: &[u8], bad_checksum: bool) -> Self {
        let server = MockServer::start();
        let name = format!("elvm-v{version}-{ELVM_TARGET}");
        let tar_name = format!("{name}.tar.gz");
        let tar_url = server.url(format!("/download/{tar_name}"));
        let sha_url = server.url(format!("/download/{tar_name}.sha256"));

        let release = format!(
            r#"{{"tag_name":"v{version}","assets":[
                {{"name":"{tar_name}","browser_download_url":"{tar_url}"}},
                {{"name":"{tar_name}.sha256","browser_download_url":"{sha_url}"}}
            ]}}"#,
        );
        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/getelephc/elvm/releases/latest");
            then.status(200)
                .header("content-type", "application/json")
                .body(release.clone());
        });

        server.mock(|when, then| {
            when.method(GET).path(format!("/download/{tar_name}"));
            then.status(200).body(tarball);
        });

        let checksum_body = if bad_checksum {
            format!("{}  {tar_name}\n", "0".repeat(64))
        } else {
            format!("{}  {tar_name}\n", sha256_hex(tarball))
        };
        server.mock(|when, then| {
            when.method(GET)
                .path(format!("/download/{tar_name}.sha256"));
            then.status(200).body(checksum_body.clone());
        });

        Self { server }
    }
}

fn elvm(sandbox: &Sandbox, upstream: &Upstream) -> Command {
    let mut cmd = Command::cargo_bin("elvm").unwrap();
    cmd.env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .env("ELVM_GITHUB_API", upstream.server.base_url())
        .env("ELVM_TARGET", ELVM_TARGET)
        .env_remove("ELEPHC_VERSION")
        .current_dir(sandbox.home());
    cmd
}

#[test]
fn self_update_replaces_the_elvm_binary() {
    let sandbox = Sandbox::new();
    // Well above this crate's CARGO_PKG_VERSION so `self_update` does not
    // short-circuit on the "already the latest version" branch.
    let content = b"FAKE ELVM BINARY v99.0.0\n";
    let tarball = fake_elvm_tarball(content);
    let upstream = Upstream::start("99.0.0", &tarball, false);

    elvm(&sandbox, &upstream)
        .arg("self-update")
        .assert()
        .success();

    // Prove the replacement actually happened, not just that the command
    // exited zero: the installed bytes must be the fixture's, not whatever
    // (if anything) was there before.
    let installed = std::fs::read(sandbox.elvm_dir().join("bin/elvm")).unwrap();
    assert_eq!(installed, content);
}

#[test]
fn self_update_leaves_the_existing_binary_when_the_checksum_does_not_match() {
    let sandbox = Sandbox::new();
    let content = b"FAKE ELVM BINARY v99.0.0\n";
    let tarball = fake_elvm_tarball(content);
    let upstream = Upstream::start("99.0.0", &tarball, true);

    let bin = sandbox.elvm_dir().join("bin/elvm");
    std::fs::write(&bin, b"original elvm binary\n").unwrap();

    elvm(&sandbox, &upstream)
        .arg("self-update")
        .assert()
        .failure()
        .stderr(predicates::str::contains("checksum"));

    // The property the whole staging-then-rename ordering exists to
    // guarantee: a failed verification must never touch the binary in place.
    assert_eq!(
        std::fs::read(&bin).unwrap(),
        b"original elvm binary\n".to_vec()
    );
}
