use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::version::VersionRequest;

pub fn run(paths: &ElvmPaths, raw: &str, args: &[String]) -> anyhow::Result<()> {
    let installed = Installed::scan(paths)?;
    let name = installed
        .resolve_name(&VersionRequest::parse(raw))
        .ok_or_else(|| anyhow::anyhow!("elephc {raw} is not installed"))?;

    let binary = paths.version_dir(&name).join("elephc");
    let status = std::process::Command::new(&binary).args(args).status()?;
    std::process::exit(status.code().unwrap_or(1));
}
