/// The triple elephc names its release assets with, for this machine.
///
/// `ELVM_TARGET` overrides detection so tests can exercise the download path
/// and the unpublished-target error on any runner.
pub fn host() -> anyhow::Result<String> {
    if let Ok(value) = std::env::var("ELVM_TARGET") {
        if !value.is_empty() {
            return Ok(value);
        }
    }
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin".to_string(),
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu".to_string(),
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu".to_string(),
        (os, arch) => anyhow::bail!("unsupported platform {os}/{arch}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_returns_a_known_triple() {
        let host = host().unwrap();
        assert!(host.contains('-'), "unexpected triple: {host}");
    }
}
