//! Process exit lifecycle.

use std::io::{self, Write as _};

use anyhow::{Error, Result};
use servo_fetch::Error as FetchError;

pub(crate) fn exit_code(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(err) if is_broken_pipe(&err) => 0,
        Err(err) => {
            eprintln!("error: {err:#}");
            error_exit_code(&err)
        }
    }
}

/// Map a failure to a sysexits.
fn error_exit_code(err: &Error) -> i32 {
    match err.chain().find_map(|cause| cause.downcast_ref::<FetchError>()) {
        Some(FetchError::InvalidUrl { .. }) => sysexits::USAGE,
        Some(FetchError::Schema(_)) => sysexits::DATAERR,
        Some(FetchError::Cookies { .. }) => sysexits::NOINPUT,
        Some(FetchError::AddressNotAllowed { .. }) => sysexits::UNAVAILABLE,
        Some(
            FetchError::Engine { .. }
            | FetchError::JavaScript { .. }
            | FetchError::Screenshot { .. }
            | FetchError::Extract(_),
        ) => sysexits::SOFTWARE,
        Some(FetchError::Io(_)) => sysexits::IOERR,
        Some(FetchError::Timeout { .. }) => sysexits::TEMPFAIL,
        _ => 1,
    }
}

/// Exit codes from [`sysexits.h`](https://man.freebsd.org/cgi/man.cgi?sysexits).
mod sysexits {
    pub(super) const USAGE: i32 = 64;
    pub(super) const DATAERR: i32 = 65;
    pub(super) const NOINPUT: i32 = 66;
    pub(super) const UNAVAILABLE: i32 = 69;
    pub(super) const SOFTWARE: i32 = 70;
    pub(super) const IOERR: i32 = 74;
    pub(super) const TEMPFAIL: i32 = 75;
}

fn is_broken_pipe(err: &Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
    })
}

/// Flush stdio and terminate via `libc::_exit`, skipping SpiderMonkey's
/// static destructors that race on `pthread_mutex_destroy`.
pub(crate) fn flush_and_exit(code: i32) -> ! {
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    #[allow(unsafe_code)]
    unsafe {
        libc::_exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_is_zero() {
        assert_eq!(exit_code(Ok(())), 0);
    }

    #[test]
    fn broken_pipe_is_zero() {
        let err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe");
        assert_eq!(exit_code(Err(Error::new(err))), 0);
    }

    #[test]
    fn broken_pipe_detected_through_chain() {
        let io = io::Error::new(io::ErrorKind::BrokenPipe, "pipe");
        let err = Error::new(io).context("while writing");
        assert!(is_broken_pipe(&err));
    }

    #[test]
    fn non_broken_pipe_not_detected() {
        let err = anyhow::anyhow!("something else");
        assert!(!is_broken_pipe(&err));
    }

    #[test]
    fn maps_servo_error_to_sysexits_code() {
        let err = Error::from(servo_fetch::load_cookies("/no/such/cookies.txt").unwrap_err());
        assert_eq!(error_exit_code(&err), 66); // EX_NOINPUT
    }

    #[test]
    fn unknown_error_is_one() {
        assert_eq!(error_exit_code(&anyhow::anyhow!("boom")), 1);
    }
}
