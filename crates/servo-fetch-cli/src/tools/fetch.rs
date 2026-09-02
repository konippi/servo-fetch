//! Fetch and batch-fetch tool helpers.

use std::sync::OnceLock;

use servo_fetch::{FetchOptions, Page, VisibilityPolicy};
use servo_fetch_types::{FetchFormat, RequestOptions};
use tokio::sync::Semaphore;
use tokio::task::{JoinSet, spawn_blocking};

use super::error::{ToolError, ToolResult};
use super::options::{apply_options, content_options};
use super::render::{paginate, render_page};

const DEFAULT_MAX_CONCURRENT_FETCHES: usize = 4;
const MAX_ALLOWED_CONCURRENCY: usize = 16;

/// Process-wide gate bounding concurrent engine fetches (`SERVO_FETCH_MAX_CONCURRENCY`).
fn fetch_semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| {
        let limit = std::env::var("SERVO_FETCH_MAX_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .map_or(DEFAULT_MAX_CONCURRENT_FETCHES, |n| n.min(MAX_ALLOWED_CONCURRENCY));
        Semaphore::new(limit)
    })
}

/// Run a built fetch on the warm engine, bounded by the global fetch semaphore.
pub(crate) async fn fetch_with(opts: FetchOptions) -> ToolResult<Page> {
    let _permit = fetch_semaphore()
        .acquire()
        .await
        .map_err(|e| ToolError::internal(format!("fetch semaphore closed: {e}")))?;
    spawn_blocking(move || servo_fetch::blocking::fetch(&opts))
        .await
        .map_err(|e| ToolError::internal(e.to_string()))?
        .map_err(ToolError::from)
}

pub(crate) struct BatchSpec<'a> {
    pub urls: &'a [String],
    pub format: FetchFormat,
    pub selector: Option<&'a str>,
    pub max_len: usize,
    pub visibility: VisibilityPolicy,
    pub options: RequestOptions,
}

pub(crate) async fn batch_fetch_pages(spec: BatchSpec<'_>) -> ToolResult<Vec<(String, ToolResult<String>)>> {
    let mut set = JoinSet::new();

    // Drain started `spawn_blocking` tasks, which cannot be aborted.
    for url in spec.urls {
        let permit = match fetch_semaphore().acquire().await {
            Ok(permit) => permit,
            Err(error) => {
                let error = ToolError::internal(format!("fetch semaphore closed: {error}"));
                set.shutdown().await;
                return Err(error);
            }
        };
        let url = url.clone();
        let selector = spec.selector.map(String::from);
        let format = spec.format;
        let max_len = spec.max_len;
        let visibility = spec.visibility;
        let options = spec.options.clone();
        set.spawn_blocking(move || {
            let _permit = permit;
            let text = render_one(&url, format, selector.as_deref(), max_len, visibility, options);
            (url, text)
        });
    }

    collect_batch_tasks(set).await
}

async fn collect_batch_tasks(
    mut set: JoinSet<(String, ToolResult<String>)>,
) -> ToolResult<Vec<(String, ToolResult<String>)>> {
    let mut results = Vec::with_capacity(set.len());
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(result) => results.push(result),
            Err(error) => {
                let error = ToolError::internal(format!("batch task failed: {error}"));
                set.shutdown().await;
                return Err(error);
            }
        }
    }
    Ok(results)
}

fn render_one(
    url: &str,
    format: FetchFormat,
    selector: Option<&str>,
    max_len: usize,
    visibility: VisibilityPolicy,
    options: RequestOptions,
) -> ToolResult<String> {
    let opts = apply_options(content_options(url, format, visibility), options)?;
    let page = servo_fetch::blocking::fetch(&opts).map_err(ToolError::from)?;
    let full = render_page(&page, url, format, selector)?;
    Ok(paginate(&servo_fetch::sanitize::sanitize(&full), 0, max_len))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    use tokio::sync::oneshot;

    use super::*;

    struct PanicSignal(Option<oneshot::Sender<()>>);

    impl Drop for PanicSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn batch_task_join_failure_waits_for_started_blocking_sibling() {
        let start = Arc::new(Barrier::new(2));
        let (panicked_tx, panicked_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let mut set = JoinSet::new();
        let sibling_start = Arc::clone(&start);
        set.spawn_blocking(move || {
            sibling_start.wait();
            release_rx.blocking_recv().expect("test releases blocking sibling");
            ("blocked".to_string(), Ok("complete".to_string()))
        });
        let panic_start = Arc::clone(&start);
        set.spawn_blocking(move || -> (String, ToolResult<String>) {
            panic_start.wait();
            let _signal = PanicSignal(Some(panicked_tx));
            panic!("intentional batch task panic");
        });

        let mut collecting = Box::pin(collect_batch_tasks(set));
        panicked_rx.await.expect("failing task panicked");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut collecting)
                .await
                .is_err(),
            "collector returned while a started blocking sibling was still running"
        );

        release_tx.send(()).expect("blocking sibling still running");
        let error = collecting.await.expect_err("join failure must abort the batch");
        assert!(error.is_internal());
    }
}
