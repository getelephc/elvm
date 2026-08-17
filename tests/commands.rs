mod support;

use assert_cmd::Command;
use support::Sandbox;

fn elvm(sandbox: &Sandbox, cwd: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("elvm").unwrap();
    cmd.env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .env_remove("ELEPHC_VERSION")
        .current_dir(cwd);
    cmd
}

#[test]
fn use_writes_a_project_version_file() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    let project = sandbox.home().join("project");
    std::fs::create_dir_all(&project).unwrap();

    elvm(&sandbox, &project)
        .args(["use", "0.26.4"])
        .assert()
        .success();

    let written = std::fs::read_to_string(project.join(".elephc-version")).unwrap();
    assert_eq!(written, "0.26.4\n");
}

#[test]
fn use_global_writes_the_global_file() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");

    elvm(&sandbox, &sandbox.home())
        .args(["use", "0.26.4", "--global"])
        .assert()
        .success();

    let written = std::fs::read_to_string(sandbox.elvm_dir().join("version")).unwrap();
    assert_eq!(written, "0.26.4\n");
}

#[test]
fn use_refuses_a_version_that_is_not_installed() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    let project = sandbox.home().join("project");
    std::fs::create_dir_all(&project).unwrap();

    elvm(&sandbox, &project)
        .args(["use", "0.25.2"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not installed"))
        .stderr(predicates::str::contains("elvm install 0.25.2"));

    assert!(!project.join(".elephc-version").exists());
}

#[test]
fn use_resolves_a_prefix_to_a_concrete_version_before_writing() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.1");
    sandbox.fake_elephc("0.26.4");
    let project = sandbox.home().join("project");
    std::fs::create_dir_all(&project).unwrap();

    elvm(&sandbox, &project)
        .args(["use", "0.26"])
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(project.join(".elephc-version")).unwrap(),
        "0.26.4\n"
    );
}

#[test]
fn ls_marks_the_active_version() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    sandbox.fake_elephc("0.25.2");
    sandbox.set_global_version("0.25.2\n");

    elvm(&sandbox, &sandbox.home())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicates::str::contains("* 0.25.2"))
        .stdout(predicates::str::contains("  0.26.4"));
}

#[test]
fn which_prints_the_binary_that_would_run() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    sandbox.set_global_version("0.26.4\n");

    let expected = sandbox.elvm_dir().join("versions/0.26.4/elephc");
    elvm(&sandbox, &sandbox.home())
        .arg("which")
        .assert()
        .success()
        .stdout(predicates::str::contains(
            expected.to_string_lossy().to_string(),
        ));
}

#[test]
fn current_explains_where_the_selection_came_from() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    let project = sandbox.home().join("project");
    sandbox.write_version_file(&project, "0.26.4\n");

    elvm(&sandbox, &project)
        .arg("current")
        .assert()
        .success()
        .stdout(predicates::str::contains("0.26.4"))
        .stdout(predicates::str::contains(".elephc-version"));
}

#[test]
fn init_prints_a_path_line_for_the_shell() {
    let sandbox = Sandbox::new();

    elvm(&sandbox, &sandbox.home())
        .args(["init", "zsh"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export PATH="))
        .stdout(predicates::str::contains("/bin:$PATH"));
}
