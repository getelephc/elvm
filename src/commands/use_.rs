use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::resolve::VERSION_FILE;
use crate::version::VersionRequest;

/// Writes a selection, refusing anything not already installed so the failure
/// happens here rather than at the next compile.
pub fn run(paths: &ElvmPaths, raw: &str, global: bool) -> anyhow::Result<()> {
    let request = VersionRequest::parse(raw);
    let installed = Installed::scan(paths)?;

    let name = installed.resolve_name(&request).ok_or_else(|| {
        anyhow::anyhow!("elephc {raw} is not installed\n  install it with: elvm install {raw}")
    })?;

    let target = if global {
        paths.global_version_file()
    } else {
        let (cwd, _) = super::context()?;
        cwd.join(VERSION_FILE)
    };

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, format!("{name}\n"))?;
    println!("now using elephc {name} ({})", target.display());
    Ok(())
}
