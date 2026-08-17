use crate::paths::ElvmPaths;
use fs2::FileExt;

/// An exclusive lock held for the duration of an install, so two concurrent
/// `elvm install` runs cannot race on the same version directory.
pub struct InstallLock {
    file: std::fs::File,
}

impl InstallLock {
    pub fn acquire(paths: &ElvmPaths) -> anyhow::Result<Self> {
        std::fs::create_dir_all(paths.root())?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(paths.lock_file())?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        // Disambiguated: `std::fs::File` gained its own `unlock()` (stable
        // since 1.89), newer than this crate's MSRV. Calling through the
        // trait keeps this on `fs2`'s implementation instead.
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
