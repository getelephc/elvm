pub mod cache;
pub mod current;
pub mod exec;
pub mod init;
pub mod install;
pub mod link;
pub mod ls;
pub mod ls_remote;
pub mod uninstall;
pub mod use_;
pub mod which;

use crate::paths::ElvmPaths;
use crate::resolve::{self, Request};
use std::path::PathBuf;

/// The cwd and HOME every command resolves against.
///
/// Both are canonicalized so `find_request`'s home boundary compares equal
/// paths; on macOS `current_dir()` resolves `/var` to `/private/var` while
/// `$HOME` may not, and the boundary check would silently never match.
pub fn context() -> anyhow::Result<(PathBuf, PathBuf)> {
    let cwd = std::env::current_dir()?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.clone());
    let home = home.canonicalize().unwrap_or(home);
    Ok((cwd, home))
}

pub fn active_request(paths: &ElvmPaths) -> anyhow::Result<Option<Request>> {
    let (cwd, home) = context()?;
    resolve::find_request(paths, &cwd, &home)
}
