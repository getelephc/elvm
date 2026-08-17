use crate::errors;
use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::version::VersionRequest;

pub fn run(paths: &ElvmPaths, version: Option<&str>) -> anyhow::Result<()> {
    let installed = Installed::scan(paths)?;
    let name = match version {
        Some(raw) => installed
            .resolve_name(&VersionRequest::parse(raw))
            .ok_or_else(|| anyhow::anyhow!("elephc {raw} is not installed"))?,
        None => {
            let request = super::active_request(paths)?.ok_or_else(errors::no_version_selected)?;
            installed
                .resolve_name(&request.request)
                .ok_or_else(|| errors::not_installed(&request))?
        }
    };
    println!("{}", paths.version_dir(&name).join("elephc").display());
    Ok(())
}
