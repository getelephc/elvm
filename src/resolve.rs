use crate::paths::ElvmPaths;
use crate::version::VersionRequest;
use std::path::{Path, PathBuf};

/// Where a version request came from, for error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RequestSource {
    Env,
    File(PathBuf),
    Global(PathBuf),
}

/// A version request together with its provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Request {
    pub raw: String,
    pub request: VersionRequest,
    pub source: RequestSource,
}

pub const VERSION_FILE: &str = ".elephc-version";

impl std::fmt::Display for RequestSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Env => write!(f, "$ELEPHC_VERSION"),
            Self::File(path) | Self::Global(path) => write!(f, "{}", path.display()),
        }
    }
}

/// Finds the nearest `.elephc-version`, walking up from `cwd` and stopping
/// after `home` (or at the filesystem root when `cwd` is outside `home`).
pub fn find_version_file(cwd: &Path, home: &Path) -> Option<PathBuf> {
    let mut current = Some(cwd);
    while let Some(dir) = current {
        let candidate = dir.join(VERSION_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir == home {
            return None;
        }
        current = dir.parent();
    }
    None
}

/// Applies the resolution precedence from the spec: environment, then the
/// nearest version file, then the global selection.
#[allow(dead_code)]
pub fn find_request(paths: &ElvmPaths, cwd: &Path, home: &Path) -> anyhow::Result<Option<Request>> {
    if let Some(value) = std::env::var_os("ELEPHC_VERSION") {
        let raw = value.to_string_lossy().trim().to_string();
        if !raw.is_empty() {
            return Ok(Some(Request {
                request: VersionRequest::parse(&raw),
                raw,
                source: RequestSource::Env,
            }));
        }
    }

    if let Some(path) = find_version_file(cwd, home) {
        let raw = read_first_line(&path)?;
        if !raw.is_empty() {
            return Ok(Some(Request {
                request: VersionRequest::parse(&raw),
                raw,
                source: RequestSource::File(path),
            }));
        }
    }

    let global = paths.global_version_file();
    if global.is_file() {
        let raw = read_first_line(&global)?;
        if !raw.is_empty() {
            return Ok(Some(Request {
                request: VersionRequest::parse(&raw),
                raw,
                source: RequestSource::Global(global),
            }));
        }
    }

    Ok(None)
}

#[allow(dead_code)]
fn read_first_line(path: &Path) -> anyhow::Result<String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    Ok(contents.lines().next().unwrap_or("").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn walks_up_to_the_nearest_version_file() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let deep = home.join("project/backend/src");
        fs::create_dir_all(&deep).unwrap();
        fs::write(home.join("project/.elephc-version"), "0.26.4\n").unwrap();

        assert_eq!(
            find_version_file(&deep, home),
            Some(home.join("project/.elephc-version"))
        );
    }

    #[test]
    fn nearest_file_wins_over_an_outer_one() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let inner = home.join("project/backend");
        fs::create_dir_all(&inner).unwrap();
        fs::write(home.join("project/.elephc-version"), "0.25.2\n").unwrap();
        fs::write(inner.join(".elephc-version"), "0.26.4\n").unwrap();

        assert_eq!(
            find_version_file(&inner, home),
            Some(inner.join(".elephc-version"))
        );
    }

    #[test]
    fn the_walk_stops_at_home() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let outside = tmp.path().join(".elephc-version");
        fs::create_dir_all(&home).unwrap();
        fs::write(&outside, "0.26.4\n").unwrap();

        assert_eq!(find_version_file(&home, &home), None);
    }
}
