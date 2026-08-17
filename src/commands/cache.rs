use crate::paths::ElvmPaths;

pub fn clean(paths: &ElvmPaths) -> anyhow::Result<()> {
    let downloads = paths.downloads();
    if !downloads.exists() {
        println!("cache is already empty");
        return Ok(());
    }
    let mut removed = 0u64;
    for entry in std::fs::read_dir(&downloads)? {
        let entry = entry?;
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        std::fs::remove_file(entry.path())?;
        removed += size;
    }
    let _ = std::fs::remove_file(paths.releases_json());
    println!("freed {} MB", removed / 1_048_576);
    Ok(())
}
