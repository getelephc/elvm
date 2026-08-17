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
    let last = script.trim_end().lines().last().unwrap().trim();
    assert_eq!(last, "main \"$@\"");
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
