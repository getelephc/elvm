use crate::commands;
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
/// `execv()` replaces the process image rather than spawning a child, so
/// exit codes, signal handling (Ctrl-C during a long compile), and TTY
/// behaviour are identical to invoking elephc directly, and no elvm process
/// lingers between the shell and the compiler.
///
/// This is not what makes archive discovery work — a spawned child would
/// also report its own path from `current_exe()`. What discovery depends on
/// is the on-disk layout: the binary elvm invokes must be the one sitting
/// beside its bridge archives.
pub fn run(args: Vec<OsString>) -> anyhow::Result<Infallible> {
    let paths = ElvmPaths::from_env()?;
    let (cwd, home) = commands::context()?;

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
