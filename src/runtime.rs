//! Shared tokio runtime construction.

use std::future::Future;

use anyhow::{Context as _, Result};

/// Run `future` on a new multi-thread tokio runtime.
pub(crate) fn block_on<F: Future>(future: F) -> Result<F::Output> {
    let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
    Ok(rt.block_on(future))
}
