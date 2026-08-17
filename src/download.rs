use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

fn client() -> anyhow::Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("elvm/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

pub fn fetch_text(url: &str) -> anyhow::Result<String> {
    Ok(client()?.get(url).send()?.error_for_status()?.text()?)
}

/// Streams `url` to `dest`, hashing as it writes, and refuses to keep a file
/// whose digest does not match. Nothing downstream ever sees a bad archive.
pub fn fetch_verified(url: &str, expected_sha256: &str, dest: &Path) -> anyhow::Result<()> {
    let mut response = client()?.get(url).send()?.error_for_status()?;
    let total = response.content_length().unwrap_or(0);

    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::with_template("{bar:40} {bytes}/{total_bytes} {bytes_per_sec}")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );

    let mut file = std::fs::File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];

    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
        bar.inc(read as u64);
    }
    file.flush()?;
    bar.finish_and_clear();

    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = std::fs::remove_file(dest);
        anyhow::bail!("checksum mismatch\n  expected {expected_sha256}\n  actual   {actual}");
    }
    Ok(())
}

/// Reads the digest out of a `shasum`-style file: `<hex>  <filename>`.
pub fn parse_checksum_file(contents: &str) -> anyhow::Result<String> {
    contents
        .split_whitespace()
        .next()
        .filter(|digest| digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|digest| digest.to_string())
        .ok_or_else(|| anyhow::anyhow!("malformed checksum file"))
}
