use std::process::Command;

/// The installer must be shell-lint clean; it is executed by `curl | sh` on
/// machines we cannot debug.
#[test]
fn installer_passes_shellcheck() {
    let Ok(output) = Command::new("shellcheck")
        .args(["-s", "sh", "install.sh"])
        .output()
    else {
        eprintln!("shellcheck not installed; skipping");
        return;
    };
    assert!(
        output.status.success(),
        "shellcheck failed:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// A truncated download must not execute a partial script, so every statement
/// has to live inside a function that is only called on the final line.
#[test]
fn installer_defers_all_work_to_a_final_main_call() {
    let script = std::fs::read_to_string("install.sh").unwrap();

    // Final line must be exactly main "$@"
    let last = script.trim_end().lines().last().unwrap().trim();
    assert_eq!(last, "main \"$@\"");

    // Scan for top-level assignments outside functions.
    // Variables are assigned with NAME=value; this pattern at column 0 is a violation.
    let mut in_function = false;
    for line in script.lines() {
        // Track function boundaries by looking at column 0
        if !line.is_empty() && !line.starts_with(|c: char| c.is_whitespace()) {
            if line.ends_with(") {") {
                in_function = true;
                // Single-line function definitions have a } on the same line, so don't stay in_function
                if line.contains('}') {
                    in_function = false;
                }
            } else if line == "}" {
                in_function = false;
            }
        }

        // Skip lines that are inside functions or that have leading whitespace
        if in_function || line.is_empty() || line.starts_with(|c: char| c.is_whitespace()) {
            continue;
        }

        // At column 0, outside functions: allow only specific patterns
        // This includes single-line function definitions like: say() { ... }
        if line.starts_with('#')
            || line == "set -eu"
            || line.ends_with(") {")
            || line == "}"
            || line == "main \"$@\""
            || (line.contains("() {") && line.ends_with('}'))
        {
            continue;
        }

        // Any other pattern at column 0 is suspicious
        panic!(
            "found disallowed top-level statement: '{}'. Only shebangs, comments, 'set -eu', \
             function definitions, closing braces, and 'main \"$@\"' are allowed at column 0",
            line
        );
    }
}

/// The installer fetches the version manager and nothing else. If it ever
/// referenced elephc — a repo, a tarball, an asset — it would be pulling the
/// 68 MB compiler the user did not ask for.
#[test]
fn installer_never_fetches_the_compiler() {
    let script = std::fs::read_to_string("install.sh").unwrap();
    // Check for patterns that indicate downloading the elephc compiler:
    // elephc-v*.tar.gz tarball names, or elephc/elephc repository references.
    // The symlink name doesn't indicate fetching the compiler.
    assert!(
        !script.contains("elephc-v") && !script.contains("illegalstudio/elephc"),
        "install.sh must not fetch the elephc compiler; it installs the version manager only"
    );
}

/// The two-step onboarding is deliberate: the installer ends by telling the
/// user which command picks a compiler version.
#[test]
fn installer_points_at_the_next_step() {
    let script = std::fs::read_to_string("install.sh").unwrap();
    assert!(
        script.contains("elvm install latest"),
        "install.sh must print the command that installs a compiler"
    );
}

/// The installer executes correctly with only curl stubbed (network boundary).
/// Real tar, shasum/sha256sum run, catching bugs that stub tools would mask.
/// Tests both the fast path (latest_version) and real crypto verification.
#[test]
fn installer_executes_with_network_stub() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use hex::encode;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::TempDir::new().expect("create temp dir");
    let temp_path = temp.path();
    let work_dir = temp_path.join("work");
    std::fs::create_dir(&work_dir).expect("create work dir");

    // Build a real tarball with real content
    let binary_content = b"test-elvm-binary-content-marker";
    let tarball_path = work_dir.join("test.tar.gz");
    {
        let tar_gz = GzEncoder::new(
            std::fs::File::create(&tarball_path).expect("create tarball"),
            Compression::default(),
        );
        let mut tar = tar::Builder::new(tar_gz);

        // Add the elvm file to the archive
        let mut header = tar::Header::new_gnu();
        header.set_size(binary_content.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, "elvm", &binary_content[..])
            .expect("add to tar");
        tar.into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip");
    }

    // Compute real SHA256
    let mut file = std::fs::File::open(&tarball_path).expect("open tarball");
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).expect("hash tarball");
    let digest = encode(hasher.finalize());

    // Create bin/ directory for curl stub
    let bin_dir = temp_path.join("bin");
    std::fs::create_dir(&bin_dir).expect("create bin dir");

    // Create curl stub: only stubs network, serves real tarball and digest
    let curl_stub = bin_dir.join("curl");
    let tarball_content = std::fs::read(&tarball_path).expect("read tarball");
    let tarball_b64 = base64_encode(&tarball_content);

    // Write curl stub: handles the releases-API lookup (stdout, no -o), the
    // tarball download, and the digest file download (both via -o).
    let curl_script = format!(
        r#"#!/bin/sh
url=""
output=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "-o" ]; then
    output="$arg"
  elif [ "${{arg#-}}" = "$arg" ]; then
    url="$arg"
  fi
  prev="$arg"
done

case "$url" in
  *releases/latest*)
    printf '{{"tag_name": "v0.1.0"}}\n'
    exit 0
    ;;
  *.tar.gz)
    mkdir -p "$(dirname "$output")"
    base64 -d > "$output" << 'TARBALL_END'
{tarball_b64}
TARBALL_END
    ;;
  *.sha256)
    mkdir -p "$(dirname "$output")"
    printf '%s\n' '{digest}' > "$output"
    ;;
  *)
    printf 'curl stub: unexpected url: %s\n' "$url" >&2
    exit 1
    ;;
esac
"#,
        tarball_b64 = tarball_b64,
        digest = digest,
    );
    std::fs::write(&curl_stub, curl_script).expect("write curl stub");

    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&curl_stub, perms).expect("chmod curl stub");

    // Set up elvm installation directory
    let elvm_dir = temp_path.join("elvm");

    // Leave ELVM_VERSION unset so the default path runs: `${ELVM_VERSION:-$(latest_version "$repo")}`
    // exercises latest_version(), which the curl stub serves via its releases/latest branch above.
    let real_path = std::env::var("PATH").expect("PATH must be set");
    let output = Command::new("sh")
        .arg("install.sh")
        .arg("--no-modify-path")
        .arg("--yes")
        .env_remove("ELVM_VERSION")
        .env("ELVM_DIR", &elvm_dir)
        .env("HOME", temp_path)
        .env("PATH", format!("{}:{}", bin_dir.display(), real_path))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run installer");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify success
    assert!(
        output.status.success(),
        "installer failed:\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );

    // Verify no unbound variables or parameter errors
    assert!(
        !stderr.contains("parameter not set") && !stderr.contains("unbound variable"),
        "stderr contains variable binding error: {}",
        stderr
    );

    // Verify the binary content matches
    let binary_path = elvm_dir.join("bin/elvm");
    let installed_content = std::fs::read(&binary_path).expect("read installed binary");
    assert_eq!(
        installed_content, binary_content,
        "installed binary content does not match tarball entry"
    );

    // Verify the symlink is relative
    let symlink_path = elvm_dir.join("bin/elephc");
    let link_target = std::fs::read_link(&symlink_path).expect("read symlink");
    assert_eq!(
        link_target.to_string_lossy(),
        "elvm",
        "symlink target must be relative 'elvm'"
    );
}

/// CRITICAL 1: `[ -r /dev/tty ]` succeeds even with no controlling terminal —
/// the device node is world-readable regardless — so the `printf … >
/// /dev/tty` that followed it failed with ENXIO, and under `set -eu` that
/// aborted the whole script *after* the binary was already installed. This
/// is unreachable from a plain child process on macOS or Linux: a `Command`
/// spawned from `cargo test` normally still has a controlling terminal (or
/// none at all, in which case `/dev/tty` opens fail the same way on the host
/// too, which would make this test flaky rather than targeted). `setsid` is
/// not available on macOS and a manual double-fork is not reliably
/// achievable from Rust in-process, so this test runs install.sh inside a
/// `docker run` container instead: a container's init process has no
/// controlling terminal even without `-t`, which reproduces the field
/// report exactly (verified manually against an Alpine container before
/// this test was written). Skips outright if docker is not available.
#[test]
fn installer_succeeds_with_no_controlling_terminal() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use hex::encode;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;

    let docker_ready = Command::new("docker")
        .arg("info")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !docker_ready {
        eprintln!("docker not available/running; skipping installer_succeeds_with_no_controlling_terminal");
        return;
    }

    let temp = tempfile::TempDir::new().expect("create temp dir");
    let work = temp.path();
    let bin_dir = work.join("bin");
    let fixtures_dir = work.join("fixtures");
    let home_dir = work.join("home");
    let elvm_dir = work.join("elvm");
    for dir in [&bin_dir, &fixtures_dir, &home_dir, &elvm_dir] {
        std::fs::create_dir_all(dir).expect("create work subdir");
    }

    // A real gzip'd tarball containing a single "elvm" entry, exactly like
    // installer_executes_with_network_stub builds, but written straight to
    // disk and bind-mounted rather than embedded as base64 in the stub.
    let binary_content = b"test-elvm-binary-content-marker";
    let tarball_path = fixtures_dir.join("test.tar.gz");
    {
        let tar_gz = GzEncoder::new(
            std::fs::File::create(&tarball_path).expect("create tarball"),
            Compression::default(),
        );
        let mut tar = tar::Builder::new(tar_gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(binary_content.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, "elvm", &binary_content[..])
            .expect("add to tar");
        tar.into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip");
    }
    let mut file = std::fs::File::open(&tarball_path).expect("open tarball");
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).expect("hash tarball");
    let digest = encode(hasher.finalize());
    std::fs::write(
        fixtures_dir.join("test.tar.gz.sha256"),
        format!("{digest}  test.tar.gz\n"),
    )
    .expect("write checksum fixture");

    // A curl stub that never touches the network: it serves the fixtures
    // above by copying them into place, and the fake "latest" release tag.
    let curl_stub = bin_dir.join("curl");
    std::fs::write(
        &curl_stub,
        r#"#!/bin/sh
url=""
output=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "-o" ]; then
    output="$arg"
  elif [ "${arg#-}" = "$arg" ]; then
    url="$arg"
  fi
  prev="$arg"
done
case "$url" in
  *releases/latest*) printf '{"tag_name": "v0.1.0"}\n' ;;
  *.tar.gz) cp /fixtures/test.tar.gz "$output" ;;
  *.sha256) cp /fixtures/test.tar.gz.sha256 "$output" ;;
  *) echo "curl stub: unexpected url: $url" >&2; exit 1 ;;
esac
"#,
    )
    .expect("write curl stub");
    std::fs::set_permissions(&curl_stub, std::fs::Permissions::from_mode(0o755))
        .expect("chmod curl stub");

    let repo = env!("CARGO_MANIFEST_DIR");
    // No `-i`/`-t`: the container's init process gets no controlling
    // terminal at all regardless of the host's own tty state, which is
    // exactly the no-controlling-terminal case CI and container runs hit.
    let output = Command::new("docker")
        .args(["run", "--rm"])
        .arg("-v")
        .arg(format!("{repo}:/repo:ro"))
        .arg("-v")
        .arg(format!("{}:/work/bin:ro", bin_dir.display()))
        .arg("-v")
        .arg(format!("{}:/fixtures:ro", fixtures_dir.display()))
        .arg("-v")
        .arg(format!("{}:/home/tester", home_dir.display()))
        .arg("-v")
        .arg(format!("{}:/elvm", elvm_dir.display()))
        .args(["-e", "HOME=/home/tester"])
        .args(["-e", "ELVM_DIR=/elvm"])
        .args(["-e", "SHELL=/bin/sh"])
        .args([
            "-e",
            "PATH=/work/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        ])
        .args(["alpine:3", "sh", "/repo/install.sh"])
        .output();

    let output = match output {
        Ok(output) => output,
        Err(err) => {
            eprintln!("docker run could not be spawned ({err}); skipping");
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // A container image pull failure (e.g. no network to Docker Hub) is an
    // environment limitation, not a regression in install.sh; skip rather
    // than fail on it.
    if !output.status.success()
        && (stderr.contains("Cannot connect to the Docker daemon")
            || stderr.contains("No such image")
            || stderr.contains("pull access denied")
            || stderr.contains("i/o timeout"))
    {
        eprintln!("docker environment unavailable ({stderr}); skipping");
        return;
    }

    assert!(
        output.status.success(),
        "installer must exit 0 with no controlling tty (the CRITICAL 1 regression \
         aborted here with ENXIO after installing the binary):\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("add this line to") && stdout.contains("yourself:"),
        "installer must print the manual-PATH instructions when neither stdin nor \
         /dev/tty is usable:\nstdout: {stdout}"
    );
    assert!(
        elvm_dir.join("bin/elvm").is_file(),
        "the binary must still be installed even though the PATH prompt could not run"
    );
}

// Simple base64 encoder for embedding tarball in shell script
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b1 = chunk[0];
        let b2 = chunk.get(1).copied().unwrap_or(0);
        let b3 = chunk.get(2).copied().unwrap_or(0);

        let c1 = (b1 >> 2) as usize;
        let c2 = (((b1 & 0x03) << 4) | (b2 >> 4)) as usize;
        let c3 = if chunk.len() > 1 {
            (((b2 & 0x0f) << 2) | (b3 >> 6)) as usize
        } else {
            64
        };
        let c4 = if chunk.len() > 2 {
            (b3 & 0x3f) as usize
        } else {
            64
        };

        result.push(CHARS[c1] as char);
        result.push(CHARS[c2] as char);
        result.push(if c3 < 64 { CHARS[c3] as char } else { '=' });
        result.push(if c4 < 64 { CHARS[c4] as char } else { '=' });
    }
    result
}
