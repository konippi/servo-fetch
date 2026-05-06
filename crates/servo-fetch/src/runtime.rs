//! Shared tokio runtime for bridging servo-fetch's sync API to the async crawl
//! implementation.

use std::future::Future;
use std::sync::LazyLock;

use anyhow::{Result, bail};
use tokio::runtime::{Builder, Handle, Runtime};

/// Process-wide tokio runtime dedicated to servo-fetch's sync↔async bridge.
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_current_thread()
        .enable_all()
        .thread_name("servo-fetch-runtime")
        .build()
        .expect("failed to build servo-fetch tokio runtime")
});

/// Run `future` on the shared tokio runtime, blocking the calling thread.
pub(crate) fn block_on<F: Future>(future: F) -> Result<F::Output> {
    if Handle::try_current().is_ok() {
        bail!(
            "servo-fetch sync API cannot be called from within a Tokio runtime; \
             wrap the call in `tokio::task::spawn_blocking`"
        );
    }
    Ok(RUNTIME.block_on(future))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_future_in_sync_context() {
        let n = block_on(async { 42 }).unwrap();
        assert_eq!(n, 42);
    }

    #[test]
    fn rejects_call_from_async_context() {
        // Spin up a minimal runtime just to simulate an async caller.
        let outer = Builder::new_current_thread().enable_all().build().unwrap();
        let result = outer.block_on(async { block_on(async { 1 }) });
        assert!(result.is_err(), "should refuse to run inside a tokio runtime");
        assert!(
            result.unwrap_err().to_string().contains("Tokio runtime"),
            "error message should mention Tokio runtime"
        );
    }
}
