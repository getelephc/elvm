use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::resolve::VERSION_FILE;
use crate::version::VersionRequest;

/// Writes a selection, refusing anything not already installed so the failure
/// happens here rather than at the next compile.
pub fn run(paths: &ElvmPaths, raw: &str, global: bool) -> anyhow::Result<()> {
    let request = VersionRequest::parse(raw);
    let installed = Installed::scan(paths)?;

    let name = installed.resolve_name(&request).ok_or_else(|| {
        anyhow::anyhow!("elephc {raw} is not installed\n  install it with: elvm install {raw}")
    })?;

    let target = if global {
        paths.global_version_file()
    } else {
        // A `.elephc-version` is meant to be committed, and the whole point
        // of committing one is that everybody builds with the same compiler.
        // `nightly` cannot deliver that: it names whichever build was newest
        // when each person last installed. Said once, here, rather than
        // refused — testing `main` across a team is a legitimate thing to
        // want, it just is not a pin.
        if matches!(&request, VersionRequest::Nightly(c) if c.is_rolling()) {
            eprintln!(
                "warning: nightly moves; committing it to {VERSION_FILE} does not pin a compiler"
            );
            eprintln!("  for a build that will not change, pin a dated one: elvm ls-remote");
        }
        let (cwd, _) = super::context()?;
        cwd.join(VERSION_FILE)
    };

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, format!("{name}\n"))?;
    println!("now using elephc {name} ({})", target.display());
    Ok(())
}
