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
