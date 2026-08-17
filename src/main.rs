mod archive;
mod build;
mod cli;
mod commands;
mod download;
mod errors;
mod github;
mod install;
mod installed;
mod lock;
mod paths;
mod resolve;
mod shim;
mod target;
mod version;

use std::ffi::OsString;
use std::path::Path;

fn main() {
    let args: Vec<OsString> = std::env::args_os().collect();
    let invoked_as = args
        .first()
        .and_then(|arg| Path::new(arg).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "elvm".to_string());

    let result = if invoked_as == "elephc" {
        shim::run(args).map(|_| ())
    } else {
        run_cli()
    };

    if let Err(err) = result {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run_cli() -> anyhow::Result<()> {
    cli::run()
}
