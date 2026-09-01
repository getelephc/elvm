use crate::installed::{self, Installed};
use crate::nightly::Stamp;
use crate::paths::ElvmPaths;
use crate::table;

/// Column headings for the installed listing. `BUILD` and `PUBLISHED` are
/// filled only by nightlies — a released version is fully identified by its
/// name — so both columns disappear when no nightly is installed.
const HEADERS: [&str; 4] = ["VERSION", "BUILD", "PUBLISHED", "NOTE"];

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

    let rows: Vec<Vec<String>> = names
        .into_iter()
        .map(|name| {
            let dir = paths.version_dir(&name);
            // A nightly directory's name says nothing about which build is
            // in it — `nightly` is the same name every night — so the stamp
            // written at install time is what makes the listing mean
            // anything.
            let stamp = Stamp::read(&dir);
            let mut notes = Vec::new();
            if Some(&name) == active.as_ref() {
                notes.push("active");
            }
            if !installed::is_complete(&dir) {
                notes.push("incomplete");
            }
            vec![
                name,
                stamp
                    .as_ref()
                    .map(|s| s.version.clone())
                    .unwrap_or_default(),
                stamp
                    .as_ref()
                    .map(|s| s.published_date().to_string())
                    .unwrap_or_default(),
                notes.join(", "),
            ]
        })
        .collect();

    table::print(&HEADERS, &rows);
    Ok(())
}
