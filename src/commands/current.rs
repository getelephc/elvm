use crate::errors;
use crate::installed::Installed;
use crate::paths::ElvmPaths;

pub fn run(paths: &ElvmPaths) -> anyhow::Result<()> {
    let request = super::active_request(paths)?.ok_or_else(errors::no_version_selected)?;
    let installed = Installed::scan(paths)?;
    let name = installed
        .resolve_name(&request.request)
        .ok_or_else(|| errors::not_installed(&request))?;
    println!(
        "{name}  (requested as \"{}\" by {})",
        request.raw, request.source
    );
    Ok(())
}
