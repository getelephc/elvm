use crate::github;
use crate::installed::Installed;
use crate::paths::ElvmPaths;
use crate::target;

pub fn run(paths: &ElvmPaths) -> anyhow::Result<()> {
    let host = target::host()?;
    let releases = github::list_releases(paths, true)?;
    let installed = Installed::scan(paths)?;

    for release in releases {
        let marker = if installed.versions.contains(&release.version) {
            "*"
        } else {
            " "
        };
        let downloadable = release
            .asset(&github::tarball_name(&release.version, &host))
            .is_some();
        let note = if downloadable {
            ""
        } else {
            "  (no binary for this platform)"
        };
        println!("{marker} {}{note}", release.version);
    }
    Ok(())
}
