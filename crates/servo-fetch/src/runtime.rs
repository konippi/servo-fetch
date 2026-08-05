//! Tokio bridge for the synchronous crawl and map APIs.

use std::future::Future;
use std::sync::LazyLock;

use anyhow::{Result, bail};
use tokio::runtime::{Builder, Handle, Runtime};

/// Fallback runtime for synchronous callers outside a Tokio runtime.
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_current_thread()
        .enable_all()
        .thread_name("servo-fetch-runtime")
        .build()
        .expect("failed to build servo-fetch tokio runtime")
});

/// Run `future` to completion while blocking the caller.
pub(crate) fn block_on<F: Future>(future: F) -> Result<F::Output> {
    if let Ok(handle) = Handle::try_current() {
        return match handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => Ok(tokio::task::block_in_place(|| handle.block_on(future))),
            _ => bail!("servo-fetch sync API cannot run on a current-thread async runtime; use the async API"),
        };
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
        assert!(inner.unwrap_err().to_string().contains("async runtime"));
    }

    #[test]
    fn rejects_direct_call_from_foreign_runtime_without_panicking() {
        let outer = Builder::new_current_thread().enable_all().build().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            outer.block_on(async { block_on(async { 42 }) })
        }));
        assert!(result.is_ok(), "blocking bridge must not panic inside a Tokio runtime");
        let error = result.unwrap().unwrap_err();
        assert!(error.to_string().contains("current-thread async runtime"));
    }

    #[test]
    fn allows_direct_call_from_foreign_multithread_runtime() {
        let outer = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let expected = outer.handle().id();
        let actual = outer.block_on(async { block_on(async { Handle::current().id() }).unwrap() });
        assert_eq!(actual, expected);
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
