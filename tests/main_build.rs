mod support;

use assert_cmd::Command;
use flate2::{write::GzEncoder, Compression};
use predicates::str::contains;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use support::Sandbox;

const COMMIT: &str = "9435998e5ab163659afefafbe525be882b13f76f";
const NEXT_COMMIT: &str = "1234567890123456789012345678901234567890";
const TARGET: &str = "linux-x86_64";

fn elvm(sandbox: &Sandbox) -> Command {
    let mut cmd = Command::cargo_bin("elvm").unwrap();
    cmd.env("PATH", sandbox.home())
        .env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .env("ELVM_TARGET", "x86_64-unknown-linux-gnu")
        .env_remove("ELEPHC_VERSION")
        .current_dir(sandbox.home());
    cmd
}

fn fake_gh(sandbox: &Sandbox, script: &str) {
    let gh = sandbox.home().join("gh");
    fs::write(&gh, format!("#!/bin/sh\n{script}")).unwrap();
    support::make_executable(&gh);
}

fn serve_build(sandbox: &Sandbox, commit: &str, id: u64, checksum_valid: bool) {
    let home = sandbox.home();
    let basename = format!("elephc-dev-{TARGET}-{commit}");
    let name = format!("{basename}-1");
    let ledger = json!({
        "schema": 1,
        "latest": {
            "sha": commit, "run_id": id,
            "artifacts": {
                TARGET: {
                    "id": id, "name": name, "expired": false,
                    "created_at": "2026-11-21T17:00:47Z"
                }
            }
        }
    });
    fs::write(home.join("ledger.json"), ledger.to_string()).unwrap();

    let bin = sandbox.fake_elephc(commit);
    let dir = bin.parent().unwrap();
    let manifest = json!({"sha": commit, "target": TARGET, "profile": "debug"});
    fs::write(dir.join("build-manifest.json"), manifest.to_string()).unwrap();
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        tar.append_path_with_name(entry.path(), entry.file_name())
            .unwrap();
    }
    let bytes = tar.into_inner().unwrap().finish().unwrap();
    let checksum = if checksum_valid {
        hex::encode(Sha256::digest(&bytes))
    } else {
        "0".repeat(64)
    };
    fs::write(home.join(format!("{basename}.tar.gz")), bytes).unwrap();
    fs::write(
        home.join(format!("{basename}.tar.gz.sha256")),
        format!("{checksum}  {basename}.tar.gz\n"),
    )
    .unwrap();

    fake_gh(
        sandbox,
        &format!(
            r#"
case "$*" in
  'auth token --hostname github.com') exit 0 ;;
  'api --hostname github.com -H Accept: application/vnd.github.raw+json repos/illegalstudio/elephc/contents/artifact-retention.json?ref=ci/development-artifact-ledger')
    exec /bin/cat ledger.json ;;
esac
[ "$#" -eq 9 ] && [ "$1 $2 $3 $4 $5 $6 $7 $8" = 'run download {id} --repo github.com/illegalstudio/elephc --name {name} --dir' ] || exit 12
echo downloaded >> downloads.log
/bin/cp {basename}.tar.gz {basename}.tar.gz.sha256 "$9"
"#
        ),
    );
}

#[test]
fn main_help_explains_the_channel_and_login() {
    let sandbox = Sandbox::new();
    elvm(&sandbox)
        .args(["install", "--help"])
        .assert()
        .success()
        .stdout(contains("main"))
        .stdout(contains("gh auth login"));
}

#[test]
fn main_explains_how_to_install_gh_and_sign_in() {
    let sandbox = Sandbox::new();
    elvm(&sandbox)
        .args(["install", "main"])
        .assert()
        .failure()
        .stderr(contains("GitHub CLI (gh) is required"))
        .stderr(contains("GitHub Actions"))
        .stderr(contains("gh auth login"));
}

#[test]
fn main_explains_how_to_sign_in_when_gh_is_unauthenticated() {
    let sandbox = Sandbox::new();
    fake_gh(&sandbox, "exit 1\n");
    elvm(&sandbox)
        .args(["install", "main"])
        .assert()
        .failure()
        .stderr(contains("GitHub Actions"))
        .stderr(contains("gh auth login"));
}

#[test]
fn main_explains_how_to_sign_in_when_github_rejects_the_credentials() {
    let sandbox = Sandbox::new();
    fake_gh(
        &sandbox,
        "[ \"$1\" = auth ] && exit 0\necho 'gh: Bad credentials (HTTP 401)' >&2\nexit 1\n",
    );
    elvm(&sandbox)
        .args(["install", "main"])
        .assert()
        .failure()
        .stderr(contains("gh auth login"));
}

#[test]
fn main_installs_and_updates_only_after_verifying_the_package() {
    let sandbox = Sandbox::new();
    sandbox.write_version_file(&sandbox.home(), "main\n");
    serve_build(&sandbox, COMMIT, 123, true);
    elvm(&sandbox)
        .arg("install")
        .assert()
        .success()
        .stdout(contains("installed elephc main at 9435998e5ab1 (debug)"));
    elvm(&sandbox)
        .arg("ls")
        .assert()
        .success()
        .stdout(contains("9435998e5ab1 (debug)"));
    elvm(&sandbox)
        .args(["install", "main"])
        .assert()
        .success()
        .stdout(contains("already up to date"));
    assert_eq!(
        fs::read_to_string(sandbox.home().join("downloads.log")).unwrap(),
        "downloaded\n"
    );

    serve_build(&sandbox, NEXT_COMMIT, 124, false);
    elvm(&sandbox)
        .args(["install", "main"])
        .assert()
        .failure()
        .stderr(contains("SHA-256 verification failed"));
    assert_compiler(&sandbox, COMMIT);

    serve_build(&sandbox, NEXT_COMMIT, 124, true);
    elvm(&sandbox).args(["install", "main"]).assert().success();
    assert_compiler(&sandbox, NEXT_COMMIT);
}

fn assert_compiler(sandbox: &Sandbox, commit: &str) {
    elvm(sandbox)
        .args(["exec", "main", "--version"])
        .assert()
        .success()
        .stdout(contains(format!("fake-elephc {commit} --version")));
}

#[test]
fn main_does_not_replace_an_existing_local_link() {
    let sandbox = Sandbox::new();
    let link = sandbox.elvm_dir().join("versions/main");
    std::os::unix::fs::symlink("missing-local-build", &link).unwrap();
    fake_gh(&sandbox, "[ \"$1\" = auth ]\n");

    elvm(&sandbox)
        .args(["install", "main", "--force"])
        .assert()
        .failure()
        .stderr(contains(
            "main is already in use by a local installation or link",
        ));
    assert!(link.is_symlink());
}
