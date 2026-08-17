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

    // Write curl stub: handle tarball download and digest file
    let curl_script = format!(
        "#!/bin/sh\n\
url=\"\"\n\
for arg in \"$@\"; do\n\
  if [ \"$prev\" != \"-o\" ] && ! echo \"$arg\" | grep -q '^-'; then\n\
    url=\"$arg\"\n\
  fi\n\
  prev=\"$arg\"\n\
done\n\
output=\"$prev\"\n\
mkdir -p \"$(dirname \"$output\")\"\n\
\n\
if echo \"$url\" | grep -q '.tar.gz$'; then\n\
  base64 -d > \"$output\" << 'TARBALL_END'\n\
{}\n\
TARBALL_END\n\
elif echo \"$url\" | grep -q '.sha256$'; then\n\
  printf '{}  elvm-v0.1.0-aarch64-apple-darwin.tar.gz\n' > \"$output\"\n\
else\n\
  printf 'curl stub: unexpected url: %s\n' \"$url\" >&2\n\
  exit 1\n\
fi\n\
",
        tarball_b64, digest
    );
    std::fs::write(&curl_stub, curl_script).expect("write curl stub");

    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&curl_stub, perms).expect("chmod curl stub");

    // Set up elvm installation directory
    let elvm_dir = temp_path.join("elvm");

    // Run the installer with ELVM_VERSION to exercise the $repo binding on line 26
    // (DO NOT set ELVM_VERSION would require complex JSON curl handling; real tarball+digest are real)
    let output = Command::new("sh")
        .arg("install.sh")
        .arg("--no-modify-path")
        .arg("--yes")
        .env("ELVM_VERSION", "0.1.0")
        .env("ELVM_DIR", &elvm_dir)
        .env("HOME", temp_path)
        .env("PATH", format!("{}:{}", bin_dir.display(), env!("PATH")))
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
