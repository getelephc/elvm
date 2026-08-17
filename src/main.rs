mod installed;
mod paths;
mod resolve;
mod version;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let paths = paths::ElvmPaths::from_env()?;
    println!("{}", paths.root().display());
    Ok(())
}
