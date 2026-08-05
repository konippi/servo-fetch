//! `/health` probe subcommand.

use std::time::Duration;

use anyhow::{Context as _, Result, bail};

use crate::cli::HealthcheckArgs;

const TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn run(args: &HealthcheckArgs) -> Result<()> {
    probe(args.port)
}

fn probe(port: u16) -> Result<()> {
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(TIMEOUT))
            .build(),
    );
    let url = format!("http://127.0.0.1:{port}/health");
    match agent.get(&url).call() {
        Ok(_) => Ok(()),
        Err(ureq::Error::StatusCode(code)) => bail!("GET {url}: status {code}"),
        Err(e) => Err(e).with_context(|| format!("GET {url}")),
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
    fn probe_5xx_errors() {
        let (port, responder) =
            spawn_responder(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let result = probe(port);
        responder.join().unwrap();
        let error = result.unwrap_err();
        assert!(format!("{error}").contains("503"));
    }
}
