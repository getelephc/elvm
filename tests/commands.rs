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

#[test]
fn uninstall_removes_a_version() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");

    elvm(&sandbox, &sandbox.home())
        .args(["uninstall", "0.26.4"])
        .assert()
        .success();

    assert!(!sandbox.elvm_dir().join("versions/0.26.4").exists());
}

#[test]
fn uninstall_refuses_a_version_that_is_not_installed() {
    let sandbox = Sandbox::new();

    elvm(&sandbox, &sandbox.home())
        .args(["uninstall", "0.26.4"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not installed"));
}

#[test]
fn exec_runs_a_specific_version_without_changing_the_selection() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("0.26.4");
    sandbox.fake_elephc("0.25.2");
    sandbox.set_global_version("0.26.4\n");

    elvm(&sandbox, &sandbox.home())
        .args(["exec", "0.25.2", "--", "hello.php"])
        .assert()
        .success()
        .stdout(predicates::str::contains("fake-elephc 0.25.2 hello.php"));

    assert_eq!(
        std::fs::read_to_string(sandbox.elvm_dir().join("version")).unwrap(),
        "0.26.4\n"
    );
}

#[test]
fn cache_clean_empties_the_downloads_directory() {
    let sandbox = Sandbox::new();
    let downloads = sandbox.elvm_dir().join("cache/downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(
        downloads.join("elephc-v0.26.4-aarch64-apple-darwin.tar.gz"),
        b"x",
    )
    .unwrap();

    elvm(&sandbox, &sandbox.home())
        .args(["cache", "clean"])
        .assert()
        .success();

    assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 0);
}

#[test]
fn uninstall_removes_a_symlinked_checkout_without_deleting_the_source() {
    let sandbox = Sandbox::new();
    let checkout = sandbox.home().join("dev-checkout");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::write(checkout.join("marker"), b"keep").unwrap();
    std::os::unix::fs::symlink(&checkout, sandbox.elvm_dir().join("versions/dev")).unwrap();

    elvm(&sandbox, &sandbox.home())
        .args(["uninstall", "dev"])
        .assert()
        .success();

    assert!(!sandbox.elvm_dir().join("versions/dev").exists());
    assert!(checkout.join("marker").exists());
}

#[test]
fn cache_clean_removes_cached_release_list_even_when_downloads_missing() {
    let sandbox = Sandbox::new();
    let cache_dir = sandbox.elvm_dir().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("releases.json"), b"stale").unwrap();

    elvm(&sandbox, &sandbox.home())
        .args(["cache", "clean"])
        .assert()
        .success();

    assert!(!cache_dir.join("releases.json").exists());
}

#[test]
fn link_registers_a_checkout_as_a_named_version() {
    let sandbox = Sandbox::new();
    let checkout = sandbox.home().join("dev/elephc/target/release");
    std::fs::create_dir_all(&checkout).unwrap();
    let binary = checkout.join("elephc");
    std::fs::write(&binary, "#!/bin/sh\necho \"fake-elephc local $*\"\n").unwrap();
    support::make_executable(&binary);
    for archive in support::BRIDGE_ARCHIVES {
        std::fs::write(checkout.join(archive), b"").unwrap();
    }

    elvm(&sandbox, &sandbox.home())
        .args(["link", checkout.to_str().unwrap()])
        .assert()
        .success();

    elvm(&sandbox, &sandbox.home())
        .args(["exec", "dev", "--", "x.php"])
        .assert()
        .success()
        .stdout(predicates::str::contains("fake-elephc local x.php"));
}

#[test]
fn link_accepts_a_repository_root_and_finds_target_release() {
    let sandbox = Sandbox::new();
    let root = sandbox.home().join("dev/elephc");
    let release = root.join("target/release");
    std::fs::create_dir_all(&release).unwrap();
    let binary = release.join("elephc");
    std::fs::write(&binary, "#!/bin/sh\necho ok\n").unwrap();
    support::make_executable(&binary);
    for archive in support::BRIDGE_ARCHIVES {
        std::fs::write(release.join(archive), b"").unwrap();
    }

    elvm(&sandbox, &sandbox.home())
        .args(["link", root.to_str().unwrap(), "--as", "wip"])
        .assert()
        .success();

    let link = sandbox.elvm_dir().join("versions/wip");
    assert_eq!(std::fs::read_link(&link).unwrap(), release);
}

#[test]
fn link_refuses_a_directory_without_an_elephc_binary() {
    let sandbox = Sandbox::new();
    let empty = sandbox.home().join("empty");
    std::fs::create_dir_all(&empty).unwrap();

    elvm(&sandbox, &sandbox.home())
        .args(["link", empty.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no elephc binary"));
}

#[test]
fn build_reports_a_missing_toolchain_before_cloning() {
    let sandbox = Sandbox::new();

    elvm(&sandbox, &sandbox.home())
        .args(["install", "--build", "v0.26.4"])
        .env("PATH", "/nonexistent")
        .assert()
        .failure()
        .stderr(predicates::str::contains("cargo"));
}

#[test]
fn build_refuses_when_the_version_directory_exists() {
    let sandbox = Sandbox::new();
    sandbox.fake_elephc("main");

    elvm(&sandbox, &sandbox.home())
        .args(["install", "--build", "main"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("already installed"))
        .stderr(predicates::str::contains("--force"));
}
