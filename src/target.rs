/// The only target elephc publishes binaries for today.
pub const PUBLISHED_TARGETS: [&str; 1] = ["aarch64-apple-darwin"];

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

pub fn elephc_publishes(target: &str) -> bool {
    PUBLISHED_TARGETS.contains(&target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_macos_arm64_is_published_upstream() {
        assert!(elephc_publishes("aarch64-apple-darwin"));
        assert!(!elephc_publishes("x86_64-unknown-linux-gnu"));
        assert!(!elephc_publishes("aarch64-unknown-linux-gnu"));
    }

    #[test]
    fn host_returns_a_known_triple() {
        let host = host().unwrap();
        assert!(host.contains('-'), "unexpected triple: {host}");
    }
}
