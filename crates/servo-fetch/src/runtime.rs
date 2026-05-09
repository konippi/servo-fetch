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
    if Handle::try_current().is_ok_and(|h| h.id() == RUNTIME.handle().id()) {
        bail!("servo-fetch sync API cannot be called from within its own async runtime");
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
    fn rejects_recursive_call() {
        let inner = block_on(async { block_on(async { 1 }) }).unwrap();
        assert!(inner.is_err(), "should refuse recursive calls into its own runtime");
        assert!(inner.unwrap_err().to_string().contains("own async runtime"));
    }

    #[test]
    fn allows_call_from_spawn_blocking() {
        let outer = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let result = outer.block_on(async {
            tokio::task::spawn_blocking(|| block_on(async { 42 }).unwrap())
                .await
                .unwrap()
        });
        assert_eq!(result, 42);
    }
}
