use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

pub fn fetch_text(url: &str) -> anyhow::Result<String> {
    let client = crate::http::client()?;
    crate::http::retry(|| {
        crate::http::text(crate::http::send(client.get(url))?.error_for_status()?)
    })
}

/// Only complete, SHA-256 verified archives are published at `dest`.
/// Partials are keyed by the expected digest so a moving nightly cannot
/// accidentally resume bytes from a different build. They survive failures
/// and process restarts; `elvm cache clean` removes them with the other files.
pub fn fetch_verified(url: &str, expected_sha256: &str, dest: &Path) -> anyhow::Result<()> {
    if let Some(actual) = hash_file(dest)? {
        if actual.eq_ignore_ascii_case(expected_sha256) {
            return Ok(());
        }
        std::fs::remove_file(dest)?;
    }
    let mut name = dest.as_os_str().to_os_string();
    name.push(format!(".{}.part", expected_sha256.to_ascii_lowercase()));
    let partial = std::path::PathBuf::from(name);
    let client = crate::http::client()?;
    crate::http::retry(|| {
        // A prior process may have finished writing but not yet renamed it.
        if hash_file(&partial)?.is_some_and(|actual| actual.eq_ignore_ascii_case(expected_sha256)) {
            std::fs::rename(&partial, dest)?;
            return Ok(());
        }
        let offset = match std::fs::metadata(&partial) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        let mut request = client
            .get(url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
        if offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        let response = crate::http::send(request)?;
        let status = response.status();
        if offset > 0 && status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            std::fs::remove_file(&partial)?;
            return Err(crate::http::transient(anyhow::anyhow!(
                "server rejected the partial download; restarting"
            )));
        }
        let response = response.error_for_status()?;
        let (append, total, end) = if status == reqwest::StatusCode::PARTIAL_CONTENT {
            let range = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_content_range);
            let valid = range.filter(|&(start, end, _)| {
                offset > 0
                    && start == offset
                    && response
                        .content_length()
                        .map_or(true, |len| len == end - start + 1)
            });
            let Some((_, end, total)) = valid else {
                if partial.exists() {
                    std::fs::remove_file(&partial)?;
                }
                return Err(crate::http::transient(anyhow::anyhow!(
                    "invalid Content-Range; restarting the download"
                )));
            };
            (true, total, Some(end))
        } else if status == reqwest::StatusCode::OK {
            // A server may ignore Range. Consume its full response from byte 0.
            (false, response.content_length().unwrap_or(0), None)
        } else {
            anyhow::bail!("unexpected download response: {status}");
        };
        copy_response(
            response,
            &partial,
            append,
            total,
            if append { offset } else { 0 },
        )?;
        if let Some(end) = end {
            let len = std::fs::metadata(&partial)?.len();
            if len != end + 1 || len < total {
                return Err(crate::http::transient(anyhow::anyhow!(
                    "incomplete range response"
                )));
            }
        }
        let actual = hash_file(&partial)?.unwrap_or_default();
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            std::fs::remove_file(&partial)?;
            let error = anyhow::anyhow!(
                "checksum mismatch\n  expected {expected_sha256}\n  actual   {actual}"
            );
            // A stale/corrupt prefix gets one fresh attempt. A full response
            // with the wrong digest is an integrity failure, not a network retry.
            return Err(if append {
                crate::http::transient(error)
            } else {
                error
            });
        }
        std::fs::rename(&partial, dest)?;
        Ok(())
    })
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let (start, end, total) = (
        start.parse().ok()?,
        end.parse::<u64>().ok()?,
        total.parse().ok()?,
    );
    (start <= end && end < total).then_some((start, end, total))
}

fn copy_response(
    mut response: reqwest::blocking::Response,
    partial: &Path,
    append: bool,
    total: u64,
    offset: u64,
) -> anyhow::Result<()> {
    let bar = ProgressBar::new(total);
    bar.set_position(offset);
    bar.set_style(
        ProgressStyle::with_template("{bar:40} {bytes}/{total_bytes} {bytes_per_sec}")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(partial)?;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            // Only response reads are retryable. Disk write failures must stop.
            let read = response.read(&mut buffer).map_err(crate::http::transient)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            bar.inc(read as u64);
        }
        file.flush()?;
        Ok(())
    })();
    bar.finish_and_clear();
    result
}

/// The SHA-256 of `path`'s contents, or `None` if it does not exist yet.
fn hash_file(path: &Path) -> anyhow::Result<Option<String>> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(Some(hex::encode(hasher.finalize())))
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

#[cfg(test)]
#[path = "download_tests.rs"]
mod tests;
