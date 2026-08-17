mod support;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
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
        .stderr(predicates::str::contains("not installed"))
        .stderr(predicates::str::contains("elvm ls"));
}

/// IMPORTANT 5: every resolution failure names the fix, per spec §4.4. These
/// three commands used to hand-roll a bare "elephc X is not installed" with
/// no fix line at all.
#[test]
fn which_names_the_fix_for_a_version_that_is_not_installed() {
    let sandbox = Sandbox::new();

    elvm(&sandbox, &sandbox.home())
        .args(["which", "0.26.4"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not installed"))
        .stderr(predicates::str::contains("elvm install 0.26.4"));
}

#[test]
fn exec_names_the_fix_for_a_version_that_is_not_installed() {
    let sandbox = Sandbox::new();

    elvm(&sandbox, &sandbox.home())
        .args(["exec", "0.26.4", "--", "x.php"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not installed"))
        .stderr(predicates::str::contains("elvm install 0.26.4"));
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

/// IMPORTANT 2: a `versions/<alias>` symlink whose target has been deleted
/// used to become invisible to `Installed::scan` (`entry.path().is_dir()`
/// follows symlinks, so a broken one reads as "not a directory" and is
/// skipped). `ls` reported "no versions installed", `doctor` agreed, and
/// `uninstall dev` said "dev is not installed" while `link ... --as dev`
/// said "dev already exists" — a loop with no exit except deleting the
/// symlink by hand. This pins the whole recovery cycle: link, delete the
/// target out from under it, and confirm `ls` still lists the name and
/// `doctor` still reports it before `uninstall` removes it for good.
#[test]
fn a_dangling_linked_checkout_stays_visible_and_recoverable() {
    let sandbox = Sandbox::new();
    let checkout = sandbox.home().join("dev-checkout");
    std::fs::create_dir_all(&checkout).unwrap();
    let binary = checkout.join("elephc");
    std::fs::write(&binary, "#!/bin/sh\necho ok\n").unwrap();
    support::make_executable(&binary);
    for archive in support::BRIDGE_ARCHIVES {
        std::fs::write(checkout.join(archive), b"").unwrap();
    }

    elvm(&sandbox, &sandbox.home())
        .args(["link", checkout.to_str().unwrap(), "--as", "dev"])
        .assert()
        .success();

    let link = sandbox.elvm_dir().join("versions/dev");
    assert!(
        std::fs::symlink_metadata(&link).is_ok(),
        "the link must exist before its target is deleted"
    );

    // Delete what the symlink points at, leaving it dangling.
    std::fs::remove_dir_all(&checkout).unwrap();
    assert!(
        std::fs::symlink_metadata(&link).is_ok(),
        "the dangling symlink itself must still be on disk"
    );

    // `ls` must still list the alias rather than reporting nothing installed.
    elvm(&sandbox, &sandbox.home())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicates::str::contains("dev"));

    // `doctor` must report it too, not stay silent about a broken alias —
    // and its fix command must be alias-appropriate. `elvm install dev
    // --force` cannot work (there is no published release named "dev"), so
    // the suggestion must be to re-link or remove it instead.
    elvm(&sandbox, &sandbox.home())
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", sandbox.elvm_dir().join("bin").display()),
        )
        .arg("doctor")
        .assert()
        .stdout(predicates::str::contains("dev"))
        .stdout(predicates::str::contains("elvm link <path> --as dev"))
        .stdout(predicates::str::contains("elvm uninstall dev"))
        .stdout(predicates::str::contains("elvm install dev --force").not());

    // `uninstall` can now find and remove it — before this fix, the only
    // recovery was `rm ~/.elvm/versions/dev` by hand.
    elvm(&sandbox, &sandbox.home())
        .args(["uninstall", "dev"])
        .assert()
        .success();

    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "uninstall must remove the dangling symlink itself"
    );
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

/// `elvm install 0.26.4 --build v0.25.2` used to silently ignore the
/// positional version and build v0.25.2 instead. clap now refuses the
/// combination outright.
#[test]
fn install_refuses_a_version_together_with_build() {
    let sandbox = Sandbox::new();

    elvm(&sandbox, &sandbox.home())
        .args(["install", "0.26.4", "--build", "v0.25.2"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
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

#[test]
fn doctor_reports_a_missing_path_entry() {
    let sandbox = Sandbox::new();

    elvm(&sandbox, &sandbox.home())
        .env("PATH", "/usr/bin:/bin")
        .arg("doctor")
        .assert()
        .failure()
        .stdout(predicates::str::contains("is not on PATH"))
        .stdout(predicates::str::contains("elvm init"));
}

/// IMPORTANT 4: `doctor` used to check only `shim.exists()`, which follows
/// the symlink, so a shim retargeted at some *other* file that still
/// happens to exist on disk read as ✓ even though it does not point at the
/// relative `elvm` link the installer creates (§4.3). This shim resolves
/// fine — `old-elvm` is really there — but is not `elvm`.
#[test]
fn doctor_reports_a_shim_pointing_at_the_wrong_target() {
    let sandbox = Sandbox::new();
    let bin = sandbox.elvm_dir().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let stale = bin.join("old-elvm");
    std::fs::write(&stale, b"#!/bin/sh\n").unwrap();
    support::make_executable(&stale);
    std::os::unix::fs::symlink("old-elvm", bin.join("elephc")).unwrap();

    elvm(&sandbox, &sandbox.home())
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .arg("doctor")
        .assert()
        .failure()
        .stdout(predicates::str::contains("instead of elvm"))
        .stdout(predicates::str::contains("ln -sf elvm"));
}

/// IMPORTANT 4: spec §5.5's "writable ~/.elvm" check, previously missing.
#[test]
fn doctor_reports_an_unwritable_root() {
    use std::os::unix::fs::PermissionsExt;
    let sandbox = Sandbox::new();
    let root = sandbox.elvm_dir();
    let original = std::fs::metadata(&root).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_mode(0o555);
    std::fs::set_permissions(&root, readonly).unwrap();

    // Permission bits don't enforce anything for a process that can bypass
    // them (root, or some CI sandboxes); if that's the case here, restore
    // and skip rather than assert something that isn't actually true.
    let probe = root.join(".doctor-test-probe");
    let bypassed = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    if bypassed {
        std::fs::set_permissions(&root, original).unwrap();
        eprintln!(
            "write access to a 0o555 directory was not denied (likely running as root); \
             skipping doctor_reports_an_unwritable_root"
        );
        return;
    }

    elvm(&sandbox, &sandbox.home())
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        )
        .arg("doctor")
        .assert()
        .failure()
        .stdout(predicates::str::contains("is not writable"))
        .stdout(predicates::str::contains("chmod"));

    std::fs::set_permissions(&root, original).unwrap();
}

#[test]
fn doctor_reports_an_incomplete_version() {
    let sandbox = Sandbox::new();
    let dir = sandbox.elvm_dir().join("versions/0.26.4");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("elephc"), "#!/bin/sh\n").unwrap();
    support::make_executable(&dir.join("elephc"));

    elvm(&sandbox, &sandbox.home())
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", sandbox.elvm_dir().join("bin").display()),
        )
        .arg("doctor")
        .assert()
        .stdout(predicates::str::contains("0.26.4"))
        .stdout(predicates::str::contains("bridge archive"));
}

#[test]
fn doctor_warns_when_another_elephc_shadows_the_shim() {
    let sandbox = Sandbox::new();
    let brew = sandbox.dir.path().join("brew/bin");
    std::fs::create_dir_all(&brew).unwrap();
    std::fs::write(brew.join("elephc"), "#!/bin/sh\n").unwrap();
    support::make_executable(&brew.join("elephc"));

    elvm(&sandbox, &sandbox.home())
        .env(
            "PATH",
            format!(
                "{}:{}",
                brew.display(),
                sandbox.elvm_dir().join("bin").display()
            ),
        )
        .arg("doctor")
        .assert()
        .stdout(predicates::str::contains(
            "comes before the shim and will shadow it",
        ));
}
