use crate::paths::ElvmPaths;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "elvm",
    version,
    about = "Version manager for the elephc compiler"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Install a version. With no argument, read .elephc-version.
    Install {
        version: Option<String>,
        /// Build from source at a tag, branch, or commit instead of downloading.
        #[arg(long, value_name = "REF")]
        build: Option<String>,
        /// Reinstall over an existing version directory.
        #[arg(long)]
        force: bool,
    },
    /// Select a version for this project, or globally.
    Use {
        version: String,
        #[arg(long)]
        global: bool,
    },
    /// List installed versions.
    Ls,
    /// List versions published upstream.
    LsRemote,
    /// Remove an installed version.
    Uninstall { version: String },
    /// Register a local checkout as a named version.
    Link {
        path: String,
        #[arg(long, default_value = "dev")]
        as_: String,
    },
    /// Run one command with a specific version, changing nothing.
    Exec {
        version: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Print the path of the binary that would run.
    Which { version: Option<String> },
    /// Show the active version and where the selection came from.
    Current,
    /// Diagnose the environment.
    Doctor,
    /// Print the PATH line for a shell profile.
    Init { shell: String },
    /// Update elvm itself.
    SelfUpdate,
    /// Manage the download cache.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
}

#[derive(Subcommand)]
pub enum CacheCommand {
    /// Delete downloaded tarballs.
    Clean,
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let paths = ElvmPaths::from_env()?;

    match cli.command {
        Command::Use { version, global } => crate::commands::use_::run(&paths, &version, global),
        Command::Ls => crate::commands::ls::run(&paths),
        Command::Which { version } => crate::commands::which::run(&paths, version.as_deref()),
        Command::Current => crate::commands::current::run(&paths),
        Command::Init { shell } => crate::commands::init::run(&paths, &shell),
        _ => anyhow::bail!("not implemented yet"),
    }
}
