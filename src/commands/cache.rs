use crate::paths::ElvmPaths;

pub fn clean(paths: &ElvmPaths) -> anyhow::Result<()> {
    let downloads = paths.downloads();
    let mut removed = 0u64;
    if downloads.exists() {
        for entry in std::fs::read_dir(&downloads)? {
            let entry = entry?;
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            std::fs::remove_file(entry.path())?;
            removed += size;
        }
    }
    let _ = std::fs::remove_file(paths.releases_json());
    if removed == 0 && !paths.releases_json().exists() {
        println!("cache is already empty");
    } else {
        println!("freed {} MB", removed / 1_048_576);
    }
    Ok(())
}
