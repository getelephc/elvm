use crate::github::REPO;
use crate::lock::InstallLock;
use crate::paths::ElvmPaths;
use crate::{archive, download, install, installed, target};
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Output, Stdio};

pub const CHANNEL: &str = "main";
const STAMP_FILE: &str = ".elvm-main.json";
const LEDGER: &str = "contents/artifact-retention.json?ref=ci/development-artifact-ledger";
const AUTH_GUIDANCE: &str =
    "elvm install main downloads a development build from GitHub Actions.\n  \
     Run gh auth login --hostname github.com to sign in.\n  \
     Then retry elvm install main.";

#[derive(Deserialize)]
struct Ledger {
    schema: u32,
    latest: Option<Build>,
}

#[derive(Deserialize)]
struct Build {
    sha: String,
    run_id: u64,
    artifacts: HashMap<String, Artifact>,
}

#[derive(Deserialize)]
struct Artifact {
    id: u64,
    name: String,
    created_at: String,
    expired: bool,
}

#[derive(Deserialize)]
struct Manifest {
    sha: String,
    target: String,
    profile: String,
}

#[derive(Deserialize, Serialize)]
pub struct Stamp {
    commit: String,
    artifact_id: u64,
    profile: String,
    created_at: String,
}

impl Stamp {
    pub fn read(dir: &Path) -> Option<Self> {
        let bytes = fs::read(dir.join(STAMP_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn summary(&self) -> String {
        let short: String = self.commit.chars().take(12).collect();
        format!("{short} ({})", self.profile)
    }

    pub fn date(&self) -> &str {
        self.created_at
            .split_once('T')
            .map_or(&self.created_at, |(date, _)| date)
    }
}

pub fn install(paths: &ElvmPaths, force: bool) -> anyhow::Result<()> {
    require_auth()?;
    let host = target::host()?;
    let target = match host.as_str() {
        "aarch64-apple-darwin" => "macos-aarch64",
        "aarch64-unknown-linux-gnu" => "linux-aarch64",
        "x86_64-unknown-linux-gnu" => "linux-x86_64",
        _ => anyhow::bail!("no elephc main build for {host}"),
    };

    paths.ensure_dirs()?;
    let _lock = InstallLock::acquire(paths)?;
    let destination = paths.version_dir(CHANNEL);
    let current = match fs::symlink_metadata(&destination) {
        Ok(info) => {
            let stamp = info.is_dir().then(|| Stamp::read(&destination)).flatten();
            Some(stamp.context(
                "main is already in use by a local installation or link.\n  \
                 Run elvm uninstall main first if you want to switch to the GitHub Actions build.",
            )?)
        }
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => return Err(err).context("checking the existing main installation"),
    };

    let output = gh()
        .args([
            "api",
            "--hostname",
            "github.com",
            "-H",
            "Accept: application/vnd.github.raw+json",
            &format!("repos/{REPO}/{LEDGER}"),
        ])
        .output()?;
    let ledger: Ledger =
        serde_json::from_slice(&checked_output(output, "read the main build index")?)
            .context("invalid elephc main build index")?;
    ensure!(
        ledger.schema == 1,
        "unsupported elephc main build index schema"
    );
    let build = ledger
        .latest
        .context("no complete elephc main build has been published yet")?;
    ensure!(
        build.sha.len() == 40 && build.sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid commit in the elephc main build index"
    );
    let artifact = build
        .artifacts
        .get(target)
        .filter(|a| !a.expired)
        .with_context(|| format!("no available elephc main build for {host}"))?;
    let basename = format!("elephc-dev-{target}-{}", build.sha);
    ensure!(
        artifact
            .name
            .strip_prefix(&format!("{basename}-"))
            .is_some_and(|attempt| {
                !attempt.is_empty() && attempt.bytes().all(|b| b.is_ascii_digit())
            }),
        "unexpected artifact name in the elephc main build index"
    );

    if let Some(stamp) = current {
        if !force && stamp.artifact_id == artifact.id && installed::is_complete(&destination) {
            println!("elephc main is already up to date at {}", stamp.summary());
            return Ok(());
        }
    }

    let downloaded = tempfile::Builder::new()
        .prefix("main-download")
        .tempdir_in(paths.tmp())?;
    println!("downloading elephc main ({})", &build.sha[..12]);
    let output = gh()
        .args(["run", "download", &build.run_id.to_string(), "--repo"])
        .arg(format!("github.com/{REPO}"))
        .args(["--name", &artifact.name, "--dir"])
        .arg(downloaded.path())
        .output()?;
    checked_output(output, "download the main build")?;

    let tarball = downloaded.path().join(format!("{basename}.tar.gz"));
    let checksum = fs::read_to_string(downloaded.path().join(format!("{basename}.tar.gz.sha256")))
        .context("reading the checksum from the main build artifact")?;
    let expected = download::parse_checksum_file(&checksum)?;
    ensure!(
        download::hash_file(&tarball)?.is_some_and(|actual| actual.eq_ignore_ascii_case(&expected)),
        "SHA-256 verification failed for the elephc main tarball"
    );

    let staging = tempfile::Builder::new()
        .prefix("install")
        .tempdir_in(paths.tmp())?;
    archive::extract_tar_gz(&tarball, staging.path())?;
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(staging.path().join("build-manifest.json"))?)
            .context("invalid manifest in the elephc main tarball")?;
    ensure!(
        manifest.sha == build.sha && manifest.target == target,
        "the elephc main tarball is for a different commit or platform"
    );
    let stamp = Stamp {
        commit: build.sha,
        artifact_id: artifact.id,
        profile: manifest.profile,
        created_at: artifact.created_at.clone(),
    };
    fs::write(staging.path().join(STAMP_FILE), serde_json::to_vec(&stamp)?)?;
    install::install_staged(
        &destination,
        staging,
        "the downloaded main archive is missing the elephc binary or its bridge archives",
    )?;
    println!("installed elephc main at {}", stamp.summary());
    Ok(())
}

fn gh() -> Command {
    let mut cmd = Command::new("gh");
    cmd.env("GH_PROMPT_DISABLED", "1");
    cmd
}

fn require_auth() -> anyhow::Result<()> {
    // Check whether gh has credentials for github.com without reading or
    // logging the token. GitHub validates it on the next request.
    let status = gh()
        .args(["auth", "token", "--hostname", "github.com"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => anyhow::bail!("{AUTH_GUIDANCE}"),
        Err(err) if err.kind() == ErrorKind::NotFound => anyhow::bail!(
            "GitHub CLI (gh) is required. Install it from https://cli.github.com/\n  {AUTH_GUIDANCE}"
        ),
        Err(err) => Err(err).context("running GitHub CLI (gh)"),
    }
}

fn checked_output(output: Output, action: &str) -> anyhow::Result<Vec<u8>> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() == Some(4) || stderr.contains("HTTP 401") {
        anyhow::bail!("{AUTH_GUIDANCE}");
    }
    anyhow::bail!(
        "GitHub CLI could not {action} ({})\n  {}",
        output.status,
        stderr.trim()
    )
}
