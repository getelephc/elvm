use crate::paths::ElvmPaths;
use crate::version::VersionRequest;
use semver::Version;
use std::path::Path;

/// The bridge static libraries elephc expects beside its own binary.
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

/// What is present under `versions/`.
#[derive(Debug, Default, Clone)]
pub struct Installed {
    pub versions: Vec<Version>,
    pub aliases: Vec<String>,
}

impl Installed {
    pub fn scan(paths: &ElvmPaths) -> anyhow::Result<Self> {
        let dir = paths.versions();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(err).map_err(|e| anyhow::anyhow!("reading {}: {e}", dir.display()))
            }
        };

        let mut found = Self::default();
        for entry in entries {
            let entry = entry?;
            // A symlink under `versions/` is always an alias created by
            // `elvm link`; it must count as present even when its target has
            // been deleted or moved, so a dangling link stays visible to
            // `ls`/`doctor` instead of disappearing — with `is_dir()`
            // (which follows symlinks) it would vanish silently, leaving
            // `uninstall <name>` unable to find it and `link ... --as <name>`
            // unable to reuse the name, an unrecoverable dead end. A plain
            // file (not a directory, not a symlink) is never a version.
            let file_type = entry.file_type()?;
            if !file_type.is_dir() && !file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            match Version::parse(&name) {
                Ok(version) => found.versions.push(version),
                Err(_) => found.aliases.push(name),
            }
        }
        found.versions.sort();
        found.aliases.sort();
        Ok(found)
    }

    /// The directory name under `versions/` that satisfies this request.
    pub fn resolve_name(&self, request: &VersionRequest) -> Option<String> {
        if let Some(version) = request.select(&self.versions) {
            return Some(version.to_string());
        }
        match request {
            // A nightly lives under its own tag name, which is never valid
            // semver, so `scan` files it among the aliases.
            VersionRequest::Nightly(channel) => {
                let name = channel.dir_name();
                self.aliases
                    .iter()
                    .any(|alias| alias == name)
                    .then(|| name.to_string())
            }
            VersionRequest::Alias(name) if self.aliases.contains(name) => Some(name.clone()),
            _ => None,
        }
    }
}

/// True when a version directory holds the binary and every bridge archive.
pub fn is_complete(dir: &Path) -> bool {
    dir.join("elephc").is_file() && BRIDGE_ARCHIVES.iter().all(|a| dir.join(a).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture(entries: &[&str]) -> (TempDir, ElvmPaths) {
        let tmp = TempDir::new().unwrap();
        let paths = ElvmPaths::with_root(tmp.path().to_path_buf());
        fs::create_dir_all(paths.versions()).unwrap();
        for entry in entries {
            fs::create_dir_all(paths.version_dir(entry)).unwrap();
        }
        (tmp, paths)
    }

    #[test]
    fn a_dangling_symlink_under_versions_is_still_an_alias() {
        let (_tmp, paths) = fixture(&["0.26.4"]);
        let target = paths.root().join("nowhere");
        std::os::unix::fs::symlink(&target, paths.version_dir("dev")).unwrap();
        assert!(
            !target.exists(),
            "the symlink target must not exist for this test to mean anything"
        );

        let installed = Installed::scan(&paths).unwrap();
        assert_eq!(installed.aliases, vec!["dev".to_string()]);
        assert_eq!(installed.versions, vec![Version::parse("0.26.4").unwrap()]);
    }

    #[test]
    fn a_nightly_resolves_to_its_tag_directory_and_nothing_else() {
        let (_tmp, paths) = fixture(&["0.26.4", "nightly", "nightly-20260901"]);
        let installed = Installed::scan(&paths).unwrap();

        let name = |s: &str| installed.resolve_name(&VersionRequest::parse(s));
        assert_eq!(name("nightly"), Some("nightly".to_string()));
        assert_eq!(
            name("nightly-20260901"),
            Some("nightly-20260901".to_string())
        );
        assert_eq!(name("nightly-20260831"), None);

        // The property that keeps nightlies out of release resolution: they
        // are directories under `versions/`, but not versions.
        assert_eq!(installed.versions, vec![Version::parse("0.26.4").unwrap()]);
        assert_eq!(name("latest"), Some("0.26.4".to_string()));
        assert_eq!(name("0.26"), Some("0.26.4".to_string()));
    }

    #[test]
    fn separates_semver_directories_from_aliases() {
        let (_tmp, paths) = fixture(&["0.26.4", "0.25.2", "dev"]);
        let installed = Installed::scan(&paths).unwrap();
        assert_eq!(
            installed.versions,
            vec![
                Version::parse("0.25.2").unwrap(),
                Version::parse("0.26.4").unwrap(),
            ]
        );
        assert_eq!(installed.aliases, vec!["dev".to_string()]);
    }

    #[test]
    fn missing_versions_directory_is_not_an_error() {
        let tmp = TempDir::new().unwrap();
        let paths = ElvmPaths::with_root(tmp.path().to_path_buf());
        let installed = Installed::scan(&paths).unwrap();
        assert!(installed.versions.is_empty());
        assert!(installed.aliases.is_empty());
    }

    #[test]
    fn resolves_requests_to_directory_names() {
        let (_tmp, paths) = fixture(&["0.26.4", "0.25.2", "dev"]);
        let installed = Installed::scan(&paths).unwrap();

        let name = |s: &str| installed.resolve_name(&VersionRequest::parse(s));
        assert_eq!(name("0.26.4"), Some("0.26.4".to_string()));
        assert_eq!(name("0.25"), Some("0.25.2".to_string()));
        assert_eq!(name("latest"), Some("0.26.4".to_string()));
        assert_eq!(name("dev"), Some("dev".to_string()));
        assert_eq!(name("0.24.0"), None);
        assert_eq!(name("nope"), None);
    }

    #[test]
    fn completeness_requires_the_binary_and_all_eight_archives() {
        let (_tmp, paths) = fixture(&["0.26.4"]);
        let dir = paths.version_dir("0.26.4");
        assert!(!is_complete(&dir));

        fs::write(dir.join("elephc"), b"#!/bin/sh\n").unwrap();
        for archive in BRIDGE_ARCHIVES.iter().take(7) {
            fs::write(dir.join(archive), b"").unwrap();
        }
        assert!(
            !is_complete(&dir),
            "seven archives must not count as complete"
        );

        fs::write(dir.join(BRIDGE_ARCHIVES[7]), b"").unwrap();
        assert!(is_complete(&dir));
    }
}
