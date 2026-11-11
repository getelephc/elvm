use super::*;
use crate::test_http::{Reply, Server};
use std::path::PathBuf;

const BODY: &[u8] = b"a complete verified archive";

fn digest() -> String {
    hex::encode(Sha256::digest(BODY))
}

fn partial(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(format!(".{}.part", digest()));
    PathBuf::from(name)
}

fn suffix(offset: usize) -> Reply {
    Reply::new(206, &BODY[offset..])
        .range(&format!("bytes={offset}-"))
        .header(
            "Content-Range",
            format!("bytes {offset}-{}/{}", BODY.len() - 1, BODY.len()),
        )
}

#[test]
fn interrupted_archive_resumes_without_publishing_partial_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("archive");
    let server = Server::start(vec![
        Reply::new(200, &BODY[..7]).header("Content-Length", BODY.len()),
        suffix(7),
    ]);
    fetch_verified(&server.url, &digest(), &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    assert!(!partial(&dest).exists());
    server.finish();
}

#[test]
fn permanent_error_preserves_a_partial_for_a_later_invocation() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("archive");
    std::fs::write(partial(&dest), &BODY[..7]).unwrap();
    let first = Server::start(vec![Reply::new(404, "missing").range("bytes=7-")]);
    assert!(fetch_verified(&first.url, &digest(), &dest).is_err());
    assert!(!dest.exists());
    assert_eq!(std::fs::read(partial(&dest)).unwrap(), &BODY[..7]);
    first.finish();
    let second = Server::start(vec![suffix(7)]);
    fetch_verified(&second.url, &digest(), &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    second.finish();
}

#[test]
fn a_server_ignoring_range_replaces_the_prefix_instead_of_appending() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("archive");
    std::fs::write(partial(&dest), &BODY[..7]).unwrap();
    let server = Server::start(vec![Reply::new(200, BODY).range("bytes=7-")]);
    fetch_verified(&server.url, &digest(), &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    server.finish();
}

#[test]
fn rejected_or_invalid_ranges_restart_from_zero() {
    for reply in [
        Reply::new(416, ""),
        Reply::new(206, &BODY[7..]),
        Reply::new(206, &BODY[7..]).header("Content-Range", "bytes 0-18/26"),
        Reply::new(206, &BODY[7..]).header("Content-Range", "bytes 7-25/20"),
        Reply::new(206, &BODY[7..]).header("Content-Range", "bytes 7-24/26"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("archive");
        std::fs::write(partial(&dest), &BODY[..7]).unwrap();
        let server = Server::start(vec![reply.range("bytes=7-"), Reply::new(200, BODY)]);
        fetch_verified(&server.url, &digest(), &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), BODY);
        server.finish();
    }
}

#[test]
fn a_corrupt_prefix_gets_a_fresh_download() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("archive");
    std::fs::write(partial(&dest), b"corrupt").unwrap();
    let server = Server::start(vec![suffix(7), Reply::new(200, BODY)]);
    fetch_verified(&server.url, &digest(), &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    server.finish();
}

#[test]
fn a_complete_partial_is_verified_and_promoted_without_network_access() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("archive");
    std::fs::write(partial(&dest), BODY).unwrap();
    fetch_verified("http://127.0.0.1:1", &digest(), &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    assert!(!partial(&dest).exists());
}

#[test]
fn an_old_digest_cannot_be_resumed_as_a_new_build() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("archive");
    let old_partial = dir.path().join(format!("archive.{}.part", "0".repeat(64)));
    std::fs::write(&old_partial, "old build").unwrap();
    let server = Server::start(vec![Reply::new(200, BODY)]);
    fetch_verified(&server.url, &digest(), &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    server.finish();
}

#[test]
fn a_bad_full_checksum_is_not_retried_or_cached() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("archive");
    let server = Server::start(vec![Reply::new(200, "wrong archive")]);
    let error = fetch_verified(&server.url, &digest(), &dest).unwrap_err();
    assert!(error.to_string().contains("checksum mismatch"));
    assert!(!dest.exists());
    assert!(!partial(&dest).exists());
    server.finish();
}

#[test]
fn metadata_retries_temporary_statuses_and_truncated_bodies() {
    let server = Server::start(vec![
        Reply::new(503, "unavailable").header("Retry-After", "0"),
        Reply::new(200, "partial").header("Content-Length", 100),
        Reply::new(200, "complete"),
    ]);
    assert_eq!(fetch_text(&server.url).unwrap(), "complete");
    server.finish();
}

#[test]
fn metadata_permanent_http_errors_stop_immediately() {
    for status in [401, 403, 404] {
        let server = Server::start(vec![Reply::new(status, "denied")]);
        assert!(fetch_text(&server.url).is_err());
        server.finish();
    }
}

#[test]
fn disk_errors_stop_without_retrying_the_request() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("missing/parent/archive");
    let server = Server::start(vec![Reply::new(200, BODY)]);
    assert!(fetch_verified(&server.url, &digest(), &dest).is_err());
    server.finish();
}
