#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A throwaway elvm root plus a fake HOME, for integration tests.
pub struct Sandbox {
    pub dir: TempDir,
}

impl Sandbox {
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("home")).unwrap();
        fs::create_dir_all(dir.path().join("elvm/versions")).unwrap();
        fs::create_dir_all(dir.path().join("elvm/bin")).unwrap();
        Self { dir }
    }

    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    pub fn elvm_dir(&self) -> PathBuf {
        self.dir.path().join("elvm")
    }

    /// Installs a fake elephc that echoes its version and arguments, so shim
    /// tests can assert on what was executed without a real compiler.
    pub fn fake_elephc(&self, name: &str) -> PathBuf {
        let dir = self.elvm_dir().join("versions").join(name);
        fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("elephc");
        fs::write(
            &bin,
            format!("#!/bin/sh\necho \"fake-elephc {name} $*\"\nexit 0\n"),
        )
        .unwrap();
        make_executable(&bin);
        for archive in BRIDGE_ARCHIVES {
            fs::write(dir.join(archive), b"").unwrap();
        }
        bin
    }

    pub fn write_version_file(&self, dir: &Path, contents: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(".elephc-version"), contents).unwrap();
    }

    pub fn set_global_version(&self, contents: &str) {
        fs::write(self.elvm_dir().join("version"), contents).unwrap();
    }

    /// Creates the `elephc` shim: a relative-target symlink to the elvm test
    /// binary, the same shape the installer creates. Tests invoke this path so
    /// that argv[0] dispatch is exercised for real rather than simulated.
    pub fn shim(&self) -> PathBuf {
        let bin = self.elvm_dir().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let shim = bin.join("elephc");
        if !shim.exists() {
            std::os::unix::fs::symlink(assert_cmd::cargo::cargo_bin("elvm"), &shim).unwrap();
        }
        shim
    }
}

pub const BRIDGE_ARCHIVES: [&str; 8] = [
    "libelephc_tls.a",
    "libelephc_pdo.a",
    "libelephc_crypto.a",
    "libelephc_bcmath.a",
    "libelephc_phar.a",
    "libelephc_tz.a",
    "libelephc_image.a",
    "libelephc_web.a",
];

pub fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}
