use std::path::Path;

pub fn extract_tar_gz(archive: &Path, into: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(archive)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    tar.set_preserve_permissions(true);
    tar.unpack(into)?;
    Ok(())
}
