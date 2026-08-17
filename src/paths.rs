use std::path::{Path, PathBuf};

/// Every path elvm owns, derived from a single root.
#[derive(Debug, Clone)]
pub struct ElvmPaths {
    root: PathBuf,
}

#[allow(dead_code)]
impl ElvmPaths {
    /// Reads `$ELVM_DIR`, falling back to `$HOME/.elvm`.
    pub fn from_env() -> anyhow::Result<Self> {
        let root = match std::env::var_os("ELVM_DIR") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => {
                let home = std::env::var_os("HOME")
                    .ok_or_else(|| anyhow::anyhow!("HOME is not set; set ELVM_DIR instead"))?;
                PathBuf::from(home).join(".elvm")
            }
        };
        Ok(Self::with_root(root))
    }

    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn version_dir(&self, name: &str) -> PathBuf {
        self.versions().join(name)
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn downloads(&self) -> PathBuf {
        self.cache().join("downloads")
    }

    pub fn releases_json(&self) -> PathBuf {
        self.cache().join("releases.json")
    }

    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn global_version_file(&self) -> PathBuf {
        self.root.join("version")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.root.join("lock")
    }

    /// Creates the directories elvm writes into. Callers do this before any install.
    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        for dir in [self.bin(), self.versions(), self.downloads(), self.tmp()] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_defaults_to_home_dot_elvm() {
        let paths = ElvmPaths::with_root(PathBuf::from("/home/x/.elvm"));
        assert_eq!(paths.root(), Path::new("/home/x/.elvm"));
        assert_eq!(paths.bin(), PathBuf::from("/home/x/.elvm/bin"));
        assert_eq!(
            paths.version_dir("0.26.4"),
            PathBuf::from("/home/x/.elvm/versions/0.26.4")
        );
        assert_eq!(
            paths.global_version_file(),
            PathBuf::from("/home/x/.elvm/version")
        );
    }
}
