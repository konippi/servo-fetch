//! `/health` probe subcommand.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use axum::http::StatusCode;

use crate::cli::HealthcheckArgs;

const TIMEOUT: Duration = Duration::from_secs(2);
const MAX_HEAD_BYTES: usize = 8 * 1024;
const MAX_HEADERS: usize = 32;

pub(crate) fn run(args: &HealthcheckArgs) -> Result<()> {
    probe(args.port)
}

/// Loopback probe; a full HTTP client is unnecessary for a container health check.
fn probe(port: u16) -> Result<()> {
    let url = format!("http://127.0.0.1:{port}/health");
    let status = status(port).with_context(|| format!("GET {url}"))?;
    if status.is_success() {
        Ok(())
    } else {
        bail!("GET {url}: status {}", status.as_u16())
    }
}

fn status(port: u16) -> Result<StatusCode> {
    let started = Instant::now();
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
    let remaining = TIMEOUT
        .checked_sub(started.elapsed())
        .filter(|left| !left.is_zero())
        .context("connecting consumed the probe timeout")?;
    stream.set_read_timeout(Some(remaining))?;
    stream.set_write_timeout(Some(remaining))?;
    write!(
        stream,
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;

    let mut head = [0; MAX_HEAD_BYTES];
    let mut filled = 0;
    loop {
        let read = stream.read(&mut head[filled..])?;
        if read == 0 {
            bail!("connection closed before a complete response head");
        }
        filled += read;
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut response = httparse::Response::new(&mut headers);
        if let httparse::Status::Complete(_) = response.parse(&head[..filled])? {
            let code = response.code.context("status line without a status code")?;
            return Ok(StatusCode::from_u16(code)?);
        }
        if filled == head.len() {
            bail!("response head exceeds {MAX_HEAD_BYTES} bytes");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;

    fn spawn_responder(response: &'static [u8]) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let responder = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            let mut request = Vec::with_capacity(1024);
            loop {
                let mut chunk = [0; 1024];
                let read = stream.read(&mut chunk).unwrap();
                assert_ne!(read, 0, "client closed before completing request headers");
                request.extend_from_slice(&chunk[..read]);
                assert!(
                    request.len() <= MAX_REQUEST_HEADER_BYTES,
                    "request headers exceeded test limit"
                );
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request_line_end = request
                .windows(2)
                .position(|window| window == b"\r\n")
                .expect("request line terminator");
            assert_eq!(&request[..request_line_end], b"GET /health HTTP/1.1");
            stream.write_all(response).unwrap();
            stream.flush().unwrap();
        });
        (port, responder)
    }

    #[test]
    fn probe_unreachable_port_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(probe(port).is_err());
    }

    #[test]
    fn probe_2xx_succeeds() {
        let (port, responder) = spawn_responder(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let result = probe(port);
        responder.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn probe_rejects_malformed_or_truncated_heads() {
        for response in [
            &b"garbage 200\r\n\r\n"[..],
            b"HTTP/1.1 200 OK\r\n",
            b"HTTP/1.1 20 OK\r\n\r\n",
        ] {
            let (port, responder) = spawn_responder(response);
            let result = probe(port);
            responder.join().unwrap();
            assert!(result.is_err(), "{response:?} must not report healthy");
        }
    }

    #[test]
    fn probe_5xx_errors() {
        let (port, responder) =
            spawn_responder(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let result = probe(port);
        responder.join().unwrap();
        let error = result.unwrap_err();
        assert!(format!("{error}").contains("503"));
    }
}
