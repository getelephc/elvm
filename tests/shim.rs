mod support;

use assert_cmd::Command;
use support::Sandbox;

/// Runs the binary through the real `elephc` symlink, so argv[0] dispatch is
/// exercised as it will be in production rather than simulated.
fn shim(sandbox: &Sandbox, cwd: &std::path::Path) -> Command {
    let mut cmd = Command::new(sandbox.shim());
    cmd.env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .env_remove("ELEPHC_VERSION")
        .current_dir(cwd);
    cmd
}

#[test]
fn executes_the_version_from_the_nearest_version_file() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    sandbox.fake_elephc("0.25.2");
    let project = sandbox.home().join("project");
    sandbox.write_version_file(&project, "0.25.2\n");

    shim(&sandbox, &project)
        .arg("hello.php")
        .assert()
        .success()
        .stdout(predicates::str::contains("fake-elephc 0.25.2 hello.php"));
}

#[test]
fn environment_overrides_the_version_file() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    sandbox.fake_elephc("0.25.2");
    let project = sandbox.home().join("project");
    sandbox.write_version_file(&project, "0.25.2\n");

    shim(&sandbox, &project)
        .env("ELEPHC_VERSION", "0.26.4")
        .assert()
        .success()
        .stdout(predicates::str::contains("fake-elephc 0.26.4"));
}

#[test]
fn falls_back_to_the_global_version() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    sandbox.set_global_version("0.26.4\n");

    shim(&sandbox, &sandbox.home())
        .assert()
        .success()
        .stdout(predicates::str::contains("fake-elephc 0.26.4"));
}

#[test]
fn a_prefix_in_the_version_file_selects_the_highest_installed_match() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.1");
    sandbox.fake_elephc("0.26.4");
    let project = sandbox.home().join("project");
    sandbox.write_version_file(&project, "0.26\n");

    shim(&sandbox, &project)
        .assert()
        .success()
        .stdout(predicates::str::contains("fake-elephc 0.26.4"));
}

#[test]
fn missing_version_fails_without_installing_anything() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    let project = sandbox.home().join("project");
    sandbox.write_version_file(&project, "0.25.2\n");

    shim(&sandbox, &project)
        .assert()
        .failure()
        .stderr(predicates::str::contains("elephc 0.25.2 is not installed"))
        .stderr(predicates::str::contains("elvm install 0.25.2"))
        .stderr(predicates::str::contains(".elephc-version"));

    assert!(
        !sandbox.elvm_dir().join("versions/0.25.2").exists(),
        "the shim must never install anything"
    );
}

#[test]
fn no_selection_at_all_explains_both_ways_to_set_one() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");

    shim(&sandbox, &sandbox.home())
        .assert()
        .failure()
        .stderr(predicates::str::contains("no elephc version selected"))
        .stderr(predicates::str::contains("elvm use"))
        .stderr(predicates::str::contains("--global"));
}

#[test]
fn exit_codes_propagate_from_the_compiler() {
    let sandbox = Sandbox::new();
    let bin = sandbox.fake_elephc("0.26.4");
    std::fs::write(&bin, "#!/bin/sh\nexit 42\n").unwrap();
    support::make_executable(&bin);
    sandbox.set_global_version("0.26.4\n");

    shim(&sandbox, &sandbox.home()).assert().code(42);
}

/// `shim::run` calls `CommandExt::exec()` rather than spawning a child and
/// forwarding its exit status, so that the compiler replaces the shim
/// process in place: same PID, so no elvm process lingers between the shell
/// and the compiler, and signals/exit codes/TTY behaviour are indistinguishable
/// from invoking elephc directly.
///
/// Nothing about exit codes or stdout distinguishes `exec` from a spawn (see
/// `exit_codes_propagate_from_the_compiler` above, which passes either way).
/// What does distinguish them is the process tree: under `exec`, the fake
/// compiler *is* the shim process, so its parent is whatever spawned the
/// shim — the test harness. Under a spawn, the shim process would still be
/// alive as an intervening parent, and the compiler's PPID would be the
/// shim's PID instead.
#[test]
fn the_compiler_replaces_the_shim_process_instead_of_being_spawned_by_it() {
    let sandbox = Sandbox::new();
    let bin = sandbox.fake_elephc("0.26.4");
    std::fs::write(&bin, "#!/bin/sh\necho $PPID\n").unwrap();
    support::make_executable(&bin);
    sandbox.set_global_version("0.26.4\n");

    let assert = shim(&sandbox, &sandbox.home()).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let reported_ppid: u32 = stdout
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("expected a numeric PPID on stdout, got {stdout:?}"));

    assert_eq!(
        reported_ppid,
        std::process::id(),
        "the compiler's parent should be this test process; a spawn would \
         report an intervening shim process's PID instead"
    );
}

/// `shim::run` and `commands::context()` canonicalize `cwd` and `$HOME`
/// before calling `resolve::find_request`, because that function stops its
/// upward walk on `dir == home` *string* equality, and macOS reports
/// `current_dir()` as `/private/var/...` while `$HOME` may be set to
/// `/var/...` (a symlink to the same place). Without canonicalizing both
/// sides, that comparison never matches and the walk climbs straight past
/// the user's home directory.
///
/// This plants a decoy `.elephc-version` one level above the sandbox's fake
/// `$HOME` naming a version that is never installed, then proves it is
/// never consulted: the shim must fall through to the global selection
/// instead, which only happens if the walk correctly stops at `$HOME`.
#[test]
fn the_shim_does_not_walk_past_the_home_directory_boundary() {
    let sandbox = Sandbox::new();
    sandbox.write_version_file(sandbox.dir.path(), "9.9.9\n");

    sandbox.fake_elephc("0.26.4");
    sandbox.set_global_version("0.26.4\n");

    shim(&sandbox, &sandbox.home())
        .assert()
        .success()
        .stdout(predicates::str::contains("fake-elephc 0.26.4"));
}
