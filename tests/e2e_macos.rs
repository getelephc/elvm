mod support;

use assert_cmd::Command;
use support::Sandbox;

/// Installs a real elephc through elvm and compiles a program that forces a
/// bridge archive to be linked.
///
/// If the version layout were wrong, elephc's `find_archive()` would miss
/// `libelephc_crypto.a` and this would fail with undefined symbols — which is
/// the entire reason installed versions keep the tarball's flat shape (binary
/// and `libelephc_*.a` side by side in `versions/<ver>/`). Nothing else in
/// the suite would catch a regression here, because every other test uses a
/// fake compiler.
#[test]
#[ignore = "downloads ~68 MB and requires macOS ARM64"]
fn compiles_a_bridge_dependent_program_through_the_shim() {
    let sandbox = Sandbox::new();

    Command::cargo_bin("elvm")
        .unwrap()
        .env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .args(["install", "latest"])
        .assert()
        .success();

    Command::cargo_bin("elvm")
        .unwrap()
        .env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .current_dir(sandbox.home())
        .args(["use", "latest", "--global"])
        .assert()
        .success();

    // hash() pulls in the crypto bridge, so this only compiles if
    // libelephc_crypto.a was found next to the binary.
    let source = sandbox.home().join("hash.php");
    std::fs::write(&source, r#"<?php echo hash("sha256", "abc");"#).unwrap();

    let shim = sandbox.shim();

    Command::new(&shim)
        .current_dir(sandbox.home())
        .env("ELVM_DIR", sandbox.elvm_dir())
        .env("HOME", sandbox.home())
        .arg("hash.php")
        .assert()
        .success();

    let compiled = sandbox.home().join("hash");
    let output = Command::new(&compiled).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout).to_string();
    assert_eq!(
        stdout.trim(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
