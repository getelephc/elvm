use crate::resolve::{Request, RequestSource};

/// The message a user sees when the selected version is not installed.
pub fn not_installed(request: &Request) -> anyhow::Error {
    let origin = match &request.source {
        RequestSource::Env => "  requested by $ELEPHC_VERSION".to_string(),
        RequestSource::File(path) => format!("  requested by {}", path.display()),
        RequestSource::Global(path) => format!("  requested by {}", path.display()),
    };
    anyhow::anyhow!(
        "elephc {} is not installed\n{}\n  install it with: elvm install {}",
        request.raw,
        origin,
        request.raw
    )
}

/// The message a user sees when nothing selects a version at all.
pub fn no_version_selected() -> anyhow::Error {
    anyhow::anyhow!(
        "no elephc version selected\n  \
         set one for this project:  elvm use <version>\n  \
         or set a default:          elvm use <version> --global\n  \
         see what is available:     elvm ls"
    )
}
