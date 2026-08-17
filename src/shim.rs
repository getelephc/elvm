use crate::errors;
use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::resolve;
use std::convert::Infallible;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Resolves a version and replaces this process with the real compiler.
///
/// `execv` rather than spawning a child: the running elephc must see its own
/// versioned path in `current_exe()` so it finds the bridge archives beside
/// it, and signals, exit codes, and TTY behaviour must be indistinguishable
/// from invoking elephc directly.
pub fn run(args: Vec<OsString>) -> anyhow::Result<Infallible> {
    let paths = ElvmPaths::from_env()?;
    let cwd = std::env::current_dir()?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| cwd.clone());
    let home = home.canonicalize().unwrap_or(home);

    let request =
        resolve::find_request(&paths, &cwd, &home)?.ok_or_else(errors::no_version_selected)?;

    let installed = Installed::scan(&paths)?;
    let name = installed
        .resolve_name(&request.request)
        .ok_or_else(|| errors::not_installed(&request))?;

    let binary = paths.version_dir(&name).join("elephc");
    if !binary.is_file() {
        return Err(errors::not_installed(&request));
    }

    let error = Command::new(&binary).args(&args[1..]).exec();
    Err(anyhow::anyhow!(
        "failed to execute {}: {error}",
        binary.display()
    ))
}
