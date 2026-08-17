use crate::paths::ElvmPaths;

pub fn run(paths: &ElvmPaths, shell: &str) -> anyhow::Result<()> {
    match shell {
        "sh" | "bash" | "zsh" => {
            println!("export PATH=\"{}:$PATH\"", paths.bin().display());
            Ok(())
        }
        "fish" => {
            println!("fish_add_path {}", paths.bin().display());
            Ok(())
        }
        other => anyhow::bail!("unsupported shell {other}; supported: sh, bash, zsh, fish"),
    }
}
