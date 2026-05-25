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
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    fn spawn_responder(response: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let _ = s.write_all(response);
            }
        });
        port
    }

    #[test]
    fn probe_unreachable_port_errors() {
        assert!(probe(1).is_err());
    }

    #[test]
    fn probe_2xx_succeeds() {
        let port = spawn_responder(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert!(probe(port).is_ok());
    }

    #[test]
    fn probe_5xx_errors() {
        let port =
            spawn_responder(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let err = probe(port).unwrap_err();
        assert!(format!("{err}").contains("503"));
    }
}
