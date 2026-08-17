use crate::paths::ElvmPaths;

pub fn clean(paths: &ElvmPaths) -> anyhow::Result<()> {
    let downloads = paths.downloads();
    let mut removed_bytes = 0u64;
    let mut removed_files = 0u32;
    if downloads.exists() {
        for entry in std::fs::read_dir(&downloads)? {
            let entry = entry?;
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            std::fs::remove_file(entry.path())?;
            removed_bytes += size;
            removed_files += 1;
        }
    }
    let json_removed = std::fs::remove_file(paths.releases_json()).is_ok();
    if removed_files == 0 && !json_removed {
        println!("cache is already empty");
    } else {
        println!("freed {} MB", removed_bytes / 1_048_576);
    }
    Ok(())
}
