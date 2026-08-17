use crate::download;
use crate::paths::ElvmPaths;
use crate::target;

const ELVM_REPO: &str = "illegalstudio/elvm";

/// Replaces the elvm binary in place.
///
/// The shim is a symlink to this file, so an atomic rename over it keeps the
/// shim valid — a hardlink would keep pointing at the replaced inode.
pub fn run(paths: &ElvmPaths) -> anyhow::Result<()> {
    let host = target::host()?;
    let api =
        std::env::var("ELVM_GITHUB_API").unwrap_or_else(|_| "https://api.github.com".to_string());

    let body = download::fetch_text(&format!("{api}/repos/{ELVM_REPO}/releases/latest"))?;
    let release: serde_json::Value = serde_json::from_str(&body)?;
    let tag = release["tag_name"].as_str().unwrap_or_default().to_string();
    let version = tag.trim_start_matches('v');

    if version == env!("CARGO_PKG_VERSION") {
        println!("elvm {version} is already the latest version");
        return Ok(());
    }

    let tarball = format!("elvm-v{version}-{host}.tar.gz");
    let assets = release["assets"].as_array().cloned().unwrap_or_default();
    let url_for = |name: &str| -> Option<String> {
        assets
            .iter()
            .find(|a| a["name"].as_str() == Some(name))
            .and_then(|a| a["browser_download_url"].as_str())
            .map(|s| s.to_string())
    };

    let tar_url =
        url_for(&tarball).ok_or_else(|| anyhow::anyhow!("release {tag} has no asset {tarball}"))?;
    let sha_url = url_for(&format!("{tarball}.sha256"))
        .ok_or_else(|| anyhow::anyhow!("release {tag} has no checksum for {tarball}"))?;

    let expected = download::parse_checksum_file(&download::fetch_text(&sha_url)?)?;
    paths.ensure_dirs()?;
    let staging = tempfile::Builder::new()
        .prefix("selfupdate")
        .tempdir_in(paths.tmp())?;
    let archive = staging.path().join(&tarball);
    download::fetch_verified(&tar_url, &expected, &archive)?;
    crate::archive::extract_tar_gz(&archive, staging.path())?;

    let new_binary = staging.path().join("elvm");
    if !new_binary.is_file() {
        anyhow::bail!("the downloaded archive does not contain an elvm binary");
    }
    std::fs::rename(&new_binary, paths.bin().join("elvm"))?;
    println!("updated elvm {} → {version}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
