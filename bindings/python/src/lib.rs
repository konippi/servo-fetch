//! PyO3 bindings for servo-fetch.

use pyo3::prelude::*;

mod client;
mod console;
mod crawl;
mod errors;
mod opts;
mod page;
mod schema;
mod validate;

use crate::errors::map_error;
use crate::opts::{BuildOpts, prepare};

/// Fetch, render, and extract a single URL.
#[pyfunction]
#[pyo3(signature = (url, *, timeout=None, settle=None, user_agent=None, screenshot=false, javascript=None, schema=None))]
#[allow(clippy::too_many_arguments)]
fn fetch(
    py: Python<'_>,
    url: String,
    timeout: Option<f64>,
    settle: Option<f64>,
    user_agent: Option<String>,
    screenshot: bool,
    javascript: Option<String>,
    schema: Option<Bound<'_, schema::Schema>>,
) -> PyResult<page::Page> {
    let prepared = prepare(BuildOpts {
        url,
        timeout,
        settle,
        user_agent,
        screenshot,
        javascript,
        schema,
    })?;
    let servo_page = py.detach(|| servo_fetch::fetch(prepared.opts)).map_err(map_error)?;
    Ok(page::Page::new(
        servo_page,
        prepared.url,
        prepared.screenshot_requested,
        prepared.js_requested,
    ))
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(fetch, m)?)?;
    m.add_class::<page::Page>()?;
    m.add_class::<schema::Schema>()?;
    m.add_class::<schema::Field>()?;
    m.add_class::<client::Client>()?;
    m.add_class::<console::ConsoleMessage>()?;
    m.add_class::<crawl::CrawlResult>()?;
    m.add_class::<crawl::MappedUrl>()?;
    errors::register(py, m)?;
    Ok(())
}
