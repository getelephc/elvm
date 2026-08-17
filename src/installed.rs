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
            // Follows symlinks on purpose: `elvm link` installs a symlink.
            if !entry.path().is_dir() {
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
            VersionRequest::Alias(name) if self.aliases.contains(name) => Some(name.clone()),
            _ => None,
        }
    }
}

/// True when a version directory holds the binary and every bridge archive.
#[allow(dead_code)]
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
