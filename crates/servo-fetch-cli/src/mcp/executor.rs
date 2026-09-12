//! Cancellable one-use browser-session execution for MCP tools.

use std::future::Future;
use std::pin::pin;

use futures_util::{StreamExt as _, stream};
use servo_fetch::{
    BrowserSession, BrowserSessionConfig, CrawlOptions, CrawlResult, FetchOptions, Page, SessionCancellation,
};
use tokio_util::sync::CancellationToken;

use crate::tools::{ToolError, ToolResult};

/// Result of a tool operation the MCP client may cancel mid-flight.
#[derive(Debug)]
pub(super) enum Outcome<T> {
    Completed(T),
    Cancelled,
}

enum Raced<T> {
    Finished(T),
    Interrupted(T),
}

/// Await `future`; on cancellation, signal the session so `future` observes it and still runs to completion.
async fn race<T>(
    token: &CancellationToken,
    cancellation: &SessionCancellation,
    future: impl Future<Output = T>,
) -> Raced<T> {
    let mut future = pin!(future);
    tokio::select! {
        biased;
        () = token.cancelled() => {
            cancellation.cancel();
            Raced::Interrupted(future.await)
        }
        output = &mut future => Raced::Finished(output),
    }
}

/// Fetch in a fresh isolated session.
pub(super) async fn fetch_in_session(
    config: BrowserSessionConfig,
    options: FetchOptions,
    token: &CancellationToken,
) -> Outcome<ToolResult<Page>> {
    in_session(config, token, async |session| session.fetch(&options).await).await
}

/// Crawl in a fresh isolated session.
pub(super) async fn crawl_in_session(
    config: BrowserSessionConfig,
    options: CrawlOptions,
    token: &CancellationToken,
) -> Outcome<ToolResult<Vec<CrawlResult>>> {
    in_session(config, token, async |session| session.crawl(&options).await).await
}

/// Fetch each URL in its own session, at most `concurrency` at a time, yielding in completion order.
pub(super) async fn batch_fetch_in_sessions(
    jobs: Vec<(String, BrowserSessionConfig, FetchOptions)>,
    concurrency: usize,
    token: &CancellationToken,
) -> Outcome<Vec<(String, ToolResult<Page>)>> {
    let results: Vec<_> = stream::iter(jobs)
        .map(|(url, config, options)| async move { (url, fetch_in_session(config, options, token).await) })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await;
    let mut pages = Vec::with_capacity(results.len());
    for (url, outcome) in results {
        match outcome {
            Outcome::Completed(page) => pages.push((url, page)),
            Outcome::Cancelled => return Outcome::Cancelled,
        }
    }
    Outcome::Completed(pages)
}

/// Run one operation in a fresh isolated session; cancellation kills the worker tree and always returns promptly.
async fn in_session<T>(
    config: BrowserSessionConfig,
    token: &CancellationToken,
    operation: impl AsyncFnOnce(&mut BrowserSession) -> servo_fetch::Result<T>,
) -> Outcome<ToolResult<T>> {
    if token.is_cancelled() {
        return Outcome::Cancelled;
    }
    let cancellation = SessionCancellation::new();
    let mut session = match race(
        token,
        &cancellation,
        BrowserSession::new_with_cancellation(config, &cancellation),
    )
    .await
    {
        Raced::Finished(Ok(session)) => session,
        Raced::Finished(Err(error)) => return Outcome::Completed(Err(error.into())),
        Raced::Interrupted(started) => {
            if let Ok(session) = started {
                force_close(session).await;
            }
            return Outcome::Cancelled;
        }
    };
    let result = match race(token, &cancellation, operation(&mut session)).await {
        Raced::Finished(result) => result.map_err(ToolError::from),
        Raced::Interrupted(_) => {
            force_close(session).await;
            return Outcome::Cancelled;
        }
    };
    match race(token, &cancellation, session.close()).await {
        Raced::Finished(Ok(())) => Outcome::Completed(result),
        Raced::Finished(Err(cleanup)) => Outcome::Completed(match result {
            Ok(_) => Err(cleanup.into()),
            Err(error) => {
                tracing::warn!(%cleanup, "session cleanup failed after operation error");
                Err(error)
            }
        }),
        Raced::Interrupted(closed) => {
            if let Err(error) = closed
                && !matches!(error, servo_fetch::Error::SessionCancelled)
            {
                tracing::warn!(%error, "session cleanup failed after cancellation");
            }
            Outcome::Cancelled
        }
    }
}

async fn force_close(session: BrowserSession) {
    if let Err(error) = session.force_close().await {
        tracing::warn!(%error, "session cleanup failed after cancellation");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_signals_the_session_before_draining_the_operation() {
        let token = CancellationToken::new();
        let cancellation = SessionCancellation::new();
        token.cancel();

        let raced = race(&token, &cancellation, async { cancellation.is_cancelled() }).await;
        assert!(matches!(raced, Raced::Interrupted(true)));
    }
}
