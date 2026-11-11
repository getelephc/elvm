#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct Reply {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
    range: Option<String>,
    path: Option<String>,
}

impl Reply {
    pub fn new(status: u16, body: impl AsRef<[u8]>) -> Self {
        Self {
            status,
            body: body.as_ref().to_vec(),
            headers: Vec::new(),
            range: None,
            path: None,
        }
    }

    pub fn header(mut self, name: &str, value: impl ToString) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn range(mut self, value: &str) -> Self {
        self.range = Some(value.to_string());
        self
    }

    pub fn path(mut self, value: &str) -> Self {
        self.path = Some(value.to_string());
        self
    }
}

/// Serve an exact sequence, including bodies shorter than Content-Length.
/// Each connection closes so the next request must reach the next reply.
pub struct Server {
    pub url: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(replies: Vec<Reply>) -> Self {
        Self::with_url(|_| replies)
    }

    pub fn with_url(build: impl FnOnce(&str) -> Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let replies = build(&url);
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let handle = thread::spawn(move || {
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if stopped.load(Ordering::Relaxed) {
                                return;
                            }
                            assert!(Instant::now() < deadline, "expected another HTTP request");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("accept: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                let range = request.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("range")
                        .then(|| value.trim().to_string())
                });
                assert_eq!(range, reply.range, "unexpected Range header");
                if let Some(path) = reply.path {
                    assert_eq!(
                        request.lines().next().unwrap(),
                        format!("GET {path} HTTP/1.1")
                    );
                }
                let mut response =
                    format!("HTTP/1.1 {} Test\r\nConnection: close\r\n", reply.status);
                if !reply
                    .headers
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                {
                    response.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
                }
                for (name, value) in reply.headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("\r\n");
                stream.write_all(response.as_bytes()).unwrap();
                let _ = stream.write_all(&reply.body);
            }
        });
        Self {
            url,
            stop,
            handle: Some(handle),
        }
    }

    pub fn finish(mut self) {
        self.handle.take().unwrap().join().unwrap();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
