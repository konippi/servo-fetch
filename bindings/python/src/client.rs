//! `Client` pyclass: shared defaults + per-call overrides.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use pyo3::prelude::*;

use crate::crawl::{CrawlResult, MappedUrl, strings_from_any};
use crate::errors::map_error;
use crate::opts::{BuildOpts, prepare};
use crate::page::Page;
use crate::schema::Schema;
use crate::validate;

/// Reusable client that shares timeout / settle / user_agent defaults across calls.
#[pyclass(frozen, module = "servo_fetch._native")]
pub(crate) struct Client {
    timeout: Duration,
    settle: Duration,
    user_agent: Option<String>,
}

#[pymethods]
impl Client {
    #[new]
    #[pyo3(signature = (*, timeout=30.0, settle=0.0, user_agent=None))]
    fn new(timeout: f64, settle: f64, user_agent: Option<String>) -> PyResult<Self> {
        Ok(Self {
            timeout: validate::timeout(timeout)?,
            settle: validate::settle(settle)?,
            user_agent,
        })
    }

    /// Fetch a single URL, merging client defaults with per-call overrides.
    #[pyo3(signature = (url, *, timeout=None, settle=None, screenshot=false, javascript=None, schema=None))]
    #[allow(clippy::too_many_arguments)]
    fn fetch(
        &self,
        py: Python<'_>,
        url: String,
        timeout: Option<f64>,
        settle: Option<f64>,
        screenshot: bool,
        javascript: Option<String>,
        schema: Option<Bound<'_, Schema>>,
    ) -> PyResult<Page> {
        let timeout = timeout.or(Some(self.timeout.as_secs_f64()));
        let settle = settle.or(Some(self.settle.as_secs_f64()));
        let prepared = prepare(BuildOpts {
            url,
            timeout,
            settle,
            user_agent: self.user_agent.clone(),
            screenshot,
            javascript,
            schema,
        })?;
        let page = py.detach(|| servo_fetch::fetch(prepared.opts)).map_err(map_error)?;
        Ok(Page::new(
            page,
            prepared.url,
            prepared.screenshot_requested,
            prepared.js_requested,
        ))
    }

    /// Crawl a site recursively and return all pages. Blocking; use `crawl_each` for streaming.
    #[pyo3(signature = (
        url,
        *,
        max_pages=50,
        max_depth=3,
        include=None,
        exclude=None,
        concurrency=1,
        delay_ms=0,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn crawl(
        &self,
        py: Python<'_>,
        url: String,
        max_pages: usize,
        max_depth: usize,
        include: Option<&Bound<'_, PyAny>>,
        exclude: Option<&Bound<'_, PyAny>>,
        concurrency: usize,
        delay_ms: u64,
    ) -> PyResult<Vec<CrawlResult>> {
        let opts = build_crawl_opts(
            &url,
            self.timeout,
            self.settle,
            self.user_agent.as_deref(),
            max_pages,
            max_depth,
            include,
            exclude,
            concurrency,
            delay_ms,
        )?;
        let results = py.detach(|| servo_fetch::crawl(opts)).map_err(map_error)?;
        Ok(results.into_iter().map(CrawlResult::from_core).collect())
    }

    /// Crawl a site, invoking `callback(CrawlResult)` as each page completes.
    #[pyo3(signature = (url, callback, *, max_pages=50, max_depth=3, include=None, exclude=None, concurrency=1, delay_ms=0, abort=None))]
    #[allow(clippy::too_many_arguments)]
    fn crawl_each(
        &self,
        py: Python<'_>,
        url: String,
        callback: Py<PyAny>,
        max_pages: usize,
        max_depth: usize,
        include: Option<&Bound<'_, PyAny>>,
        exclude: Option<&Bound<'_, PyAny>>,
        concurrency: usize,
        delay_ms: u64,
        abort: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let opts = build_crawl_opts(
            &url,
            self.timeout,
            self.settle,
            self.user_agent.as_deref(),
            max_pages,
            max_depth,
            include,
            exclude,
            concurrency,
            delay_ms,
        )?;

        let stop = AtomicBool::new(false);
        let cb_err: Mutex<Option<PyErr>> = Mutex::new(None);

        let result = py.detach(|| {
            servo_fetch::crawl_each(opts, |r| {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                Python::attach(|py| {
                    if let Some(ref ab) = abort {
                        if matches!(
                            ab.bind(py).call_method0("is_set").and_then(|v| v.extract::<bool>()),
                            Ok(true)
                        ) {
                            stop.store(true, Ordering::Release);
                            return;
                        }
                    }
                    let item = match Py::new(py, CrawlResult::from_core(r.clone())) {
                        Ok(i) => i,
                        Err(e) => {
                            *cb_err.lock().expect("poisoned") = Some(e);
                            stop.store(true, Ordering::Release);
                            return;
                        }
                    };
                    if let Err(e) = callback.call1(py, (item,)) {
                        *cb_err.lock().expect("poisoned") = Some(e);
                        stop.store(true, Ordering::Release);
                    }
                });
            })
        });

        if let Some(e) = cb_err.into_inner().expect("poisoned") {
            return Err(e);
        }
        result.map_err(map_error)
    }

    /// Discover URLs on a site via sitemap and link extraction (no rendering).
    #[pyo3(signature = (url, *, limit=5000, include=None, exclude=None))]
    fn map(
        &self,
        py: Python<'_>,
        url: String,
        limit: usize,
        include: Option<&Bound<'_, PyAny>>,
        exclude: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Vec<MappedUrl>> {
        validate::url(&url)?;
        let include = strings_from_any(include)?;
        let exclude = strings_from_any(exclude)?;
        let include_refs: Vec<&str> = include.iter().map(String::as_str).collect();
        let exclude_refs: Vec<&str> = exclude.iter().map(String::as_str).collect();
        let mut opts = servo_fetch::MapOptions::new(url)
            .limit(limit)
            .timeout(self.timeout.as_secs())
            .include(&include_refs)
            .exclude(&exclude_refs);
        if let Some(ua) = self.user_agent.as_deref() {
            opts = opts.user_agent(ua);
        }
        let urls = py.detach(|| servo_fetch::map(opts)).map_err(map_error)?;
        Ok(urls.into_iter().map(MappedUrl::from_core).collect())
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name = slf.get_type().qualname()?;
        let this = slf.borrow();
        Ok(format!(
            "{name}(timeout={}, settle={}, user_agent={:?})",
            this.timeout.as_secs_f64(),
            this.settle.as_secs_f64(),
            this.user_agent.as_deref().unwrap_or("")
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn build_crawl_opts(
    url: &str,
    timeout: Duration,
    settle: Duration,
    user_agent: Option<&str>,
    max_pages: usize,
    max_depth: usize,
    include: Option<&Bound<'_, PyAny>>,
    exclude: Option<&Bound<'_, PyAny>>,
    concurrency: usize,
    delay_ms: u64,
) -> PyResult<servo_fetch::CrawlOptions> {
    validate::url(url)?;
    let include = strings_from_any(include)?;
    let exclude = strings_from_any(exclude)?;
    let include_refs: Vec<&str> = include.iter().map(String::as_str).collect();
    let exclude_refs: Vec<&str> = exclude.iter().map(String::as_str).collect();
    let mut opts = servo_fetch::CrawlOptions::new(url)
        .limit(max_pages)
        .max_depth(max_depth)
        .timeout(timeout)
        .settle(settle)
        .include(&include_refs)
        .exclude(&exclude_refs)
        .concurrency(concurrency);
    opts = if delay_ms > 0 {
        opts.delay(Some(Duration::from_millis(delay_ms)))
    } else {
        opts.delay(None)
    };
    if let Some(ua) = user_agent {
        opts = opts.user_agent(ua);
    }
    Ok(opts)
}
