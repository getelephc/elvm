use crate::installed::{self, Installed};
use crate::paths::ElvmPaths;

pub fn run(paths: &ElvmPaths) -> anyhow::Result<()> {
    let installed = Installed::scan(paths)?;
    let active =
        super::active_request(paths)?.and_then(|request| installed.resolve_name(&request.request));

    if installed.versions.is_empty() && installed.aliases.is_empty() {
        println!("no versions installed — try: elvm install latest");
        return Ok(());
    }

    let mut names: Vec<String> = installed.versions.iter().map(|v| v.to_string()).collect();
    names.extend(installed.aliases.iter().cloned());

    for name in names {
        let marker = if Some(&name) == active.as_ref() {
            "*"
        } else {
            " "
        };
        let dir = paths.version_dir(&name);
        let note = if installed::is_complete(&dir) {
            ""
        } else {
            "  (incomplete)"
        };
        println!("{marker} {name}{note}");
    }
    Ok(())
}
