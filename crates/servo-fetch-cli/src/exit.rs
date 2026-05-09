//! Process exit lifecycle.

use std::io::Write as _;

pub(crate) fn exit_code(result: anyhow::Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(err) if is_broken_pipe(&err) => 0,
        Err(err) => {
            eprintln!("error: {err:#}");
            1
        }
    }
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

/// Flush stdio and terminate via `libc::_exit`, skipping SpiderMonkey's
/// static destructors that race on `pthread_mutex_destroy`.
pub(crate) fn flush_and_exit(code: i32) -> ! {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
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
        let err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe");
        assert_eq!(exit_code(Err(anyhow::Error::new(err))), 0);
    }

    #[test]
    fn other_error_is_one() {
        assert_eq!(exit_code(Err(anyhow::anyhow!("boom"))), 1);
    }

    #[test]
    fn broken_pipe_detected_through_chain() {
        let io = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe");
        let err = anyhow::Error::new(io).context("while writing");
        assert!(is_broken_pipe(&err));
    }

    #[test]
    fn non_broken_pipe_not_detected() {
        let err = anyhow::anyhow!("something else");
        assert!(!is_broken_pipe(&err));
    }
}
