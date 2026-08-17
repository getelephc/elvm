use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::version::VersionRequest;

pub fn run(paths: &ElvmPaths, raw: &str) -> anyhow::Result<()> {
    let installed = Installed::scan(paths)?;
    let name = installed
        .resolve_name(&VersionRequest::parse(raw))
        .ok_or_else(|| {
            anyhow::anyhow!("elephc {raw} is not installed\n  see what is installed: elvm ls")
        })?;

    let dir = paths.version_dir(&name);
    // A linked checkout is a symlink; removing the link must not touch the
    // user's source tree. This guard is deliberate: it makes the destructive
    // operation's intent explicit and keeps behaviour correct if the path
    // is ever resolved before removal (as canonicalize would do).
    if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
        std::fs::remove_file(&dir)?;
    } else {
        std::fs::remove_dir_all(&dir)?;
    }
    println!("removed elephc {name}");
    Ok(())
}
