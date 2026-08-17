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

/// The installer executes correctly against stubbed curl, proving variable
/// binding and control flow work. This catches runtime bugs like unbound
/// variables that text inspection cannot detect.
#[test]
fn installer_executes_with_stubbed_dependencies() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::TempDir::new().expect("create temp dir");
    let temp_path = temp.path();

    // Create bin/ directory for our stubs
    let bin_dir = temp_path.join("bin");
    std::fs::create_dir(&bin_dir).expect("create bin dir");

    // Create stub curl script that responds to API and download requests
    let curl_stub = bin_dir.join("curl");
    let mut curl_file = std::fs::File::create(&curl_stub).expect("create curl stub");
    curl_file
        .write_all(
            br#"#!/bin/sh
# Stub curl for installer testing
url=""
output_file=""
i=1
for arg in "$@"; do
    case "$arg" in
        -f|-s|-S|-L) ;;  # ignore flags
        -o)
            i=$((i+1))
            eval "output_file=\${$i}"
            ;;
        *)
            if ! echo "$arg" | grep -q '^-'; then
                url="$arg"
            fi
            ;;
    esac
    i=$((i+1))
done

case "$url" in
    *releases/latest)
        # Return API response with version 0.1.0
        printf '{"tag_name":"v0.1.0","name":"v0.1.0"}\n'
        exit 0
        ;;
    *.tar.gz)
        # Create a minimal valid gzip tar with single elvm file
        mkdir -p "$(dirname "$output_file")"
        # Create a simple tar file with an elvm marker
        printf 'test-elvm-binary' | tar czf "$output_file" -C "$(dirname "$output_file")" --transform='s,^,-,'
        exit 0
        ;;
    *.sha256)
        # Return a valid SHA256 hash
        mkdir -p "$(dirname "$output_file")"
        printf 'a665a45920422f9d417e4867efdc4fb8a04a1f3fff1fa07e998e86f7f7a27ae3\n' > "$output_file"
        exit 0
        ;;
    *)
        printf 'error: unexpected url: %s\n' "$url" >&2
        exit 1
        ;;
esac
"#,
        )
        .expect("write curl stub");
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&curl_stub, perms).expect("chmod curl stub");

    // Create stub tar to extract our test file
    let tar_stub = bin_dir.join("tar");
    let mut tar_file = std::fs::File::create(&tar_stub).expect("create tar stub");
    tar_file
        .write_all(
            br#"#!/bin/sh
# Stub tar for installer testing
# install.sh calls: tar -xzf "${tmp}/${tarball}" -C "$tmp"
# We just need to create the elvm file in the specified directory
extract_dir=""
prev=""
for arg in "$@"; do
    if [ "$prev" = "-C" ]; then
        extract_dir="$arg"
    fi
    prev="$arg"
done
if [ -n "$extract_dir" ]; then
    printf 'test-elvm-binary-content' > "$extract_dir/elvm"
fi
"#,
        )
        .expect("write tar stub");
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&tar_stub, perms).expect("chmod tar stub");

    // Create stub shasum
    let shasum_stub = bin_dir.join("shasum");
    let mut shasum_file = std::fs::File::create(&shasum_stub).expect("create shasum stub");
    shasum_file
        .write_all(
            br#"#!/bin/sh
# Stub shasum for installer testing
# Just compute real SHA256 of the file
file="${3:--}"
if [ "$1" = "-a" ] && [ "$2" = "256" ]; then
    file="$4"
fi
# Return a consistent hash for our test tarball
printf 'a665a45920422f9d417e4867efdc4fb8a04a1f3fff1fa07e998e86f7f7a27ae3  %s\n' "$file"
"#,
        )
        .expect("write shasum stub");
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&shasum_stub, perms).expect("chmod shasum stub");

    // Set up elvm installation directory
    let elvm_dir = temp_path.join("elvm");

    // Run the installer with stubbed tools
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

    // Verify the binary was installed
    let binary_path = elvm_dir.join("bin/elvm");
    assert!(
        binary_path.exists(),
        "elvm binary not found at {}",
        binary_path.display()
    );

    // Verify the symlink was created as relative
    let symlink_path = elvm_dir.join("bin/elephc");
    assert!(
        symlink_path.exists(),
        "elephc symlink not found at {}",
        symlink_path.display()
    );

    // Verify the symlink target is relative (not absolute)
    let link_target = std::fs::read_link(&symlink_path).expect("read symlink");
    assert_eq!(
        link_target.to_string_lossy(),
        "elvm",
        "symlink target should be relative 'elvm', not {}",
        link_target.display()
    );
}
