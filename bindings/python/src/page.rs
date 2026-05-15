//! `Page` pyclass over `servo_fetch::Page`.

use std::sync::OnceLock;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::console::ConsoleMessage;
use crate::errors::{EngineError, map_error};

/// A rendered web page returned by [`fetch`](crate::fetch).
#[pyclass(frozen, module = "servo_fetch._native")]
pub(crate) struct Page {
    inner: servo_fetch::Page,
    url: String,
    screenshot_requested: bool,
    js_requested: bool,
    markdown_cache: OnceLock<String>,
    extracted_cache: OnceLock<Option<Py<PyAny>>>,
}

impl Page {
    pub(crate) fn new(inner: servo_fetch::Page, url: String, screenshot_requested: bool, js_requested: bool) -> Self {
        Self {
            inner,
            url,
            screenshot_requested,
            js_requested,
            markdown_cache: OnceLock::new(),
            extracted_cache: OnceLock::new(),
        }
    }
}

#[pymethods]
impl Page {
    /// The URL that was fetched.
    #[getter]
    fn url(&self) -> &str {
        &self.url
    }

    /// Fully rendered HTML after JavaScript execution.
    #[getter]
    fn html(&self) -> &str {
        &self.inner.html
    }

    /// Plain text content (`document.body.innerText`).
    #[getter]
    fn inner_text(&self) -> &str {
        &self.inner.inner_text
    }

    /// Page title from `<title>`.
    #[getter]
    fn title(&self) -> Option<&str> {
        self.inner.title.as_deref()
    }

    /// Readable Markdown extracted from the page (lazy, cached).
    #[getter]
    fn markdown(&self, py: Python<'_>) -> PyResult<String> {
        if let Some(cached) = self.markdown_cache.get() {
            return Ok(cached.clone());
        }
        let md = py
            .detach(|| self.inner.markdown_with_url(&self.url))
            .map_err(map_error)?;
        let _ = self.markdown_cache.set(md.clone());
        Ok(md)
    }

    /// Structured data extracted via [`Schema`](crate::schema::Schema), if a schema was passed.
    #[getter]
    fn extracted(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        if let Some(cached) = self.extracted_cache.get() {
            return Ok(cached.as_ref().map(|o| o.clone_ref(py)));
        }
        let converted = match self.inner.extracted.as_ref() {
            Some(v) => Some(pythonize::pythonize(py, v)?.unbind()),
            None => None,
        };
        let _ = self.extracted_cache.set(converted);
        Ok(self
            .extracted_cache
            .get()
            .and_then(Option::as_ref)
            .map(|o| o.clone_ref(py)))
    }

    /// PNG screenshot bytes, or `None` if `screenshot=True` was not requested on `fetch()`.
    #[getter]
    fn screenshot<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        if !self.screenshot_requested {
            return None;
        }
        self.inner.screenshot_png().map(|bytes| PyBytes::new(py, bytes))
    }

    /// Result of the JavaScript expression, or `None` if `javascript=` was not requested.
    #[getter]
    fn js_result(&self) -> Option<&str> {
        if !self.js_requested {
            return None;
        }
        self.inner.js_result.as_deref()
    }

    /// Browser console messages captured during load.
    #[getter]
    fn console(&self) -> Vec<ConsoleMessage> {
        self.inner
            .console_messages
            .iter()
            .map(ConsoleMessage::from_core)
            .collect()
    }

    /// Write the screenshot PNG to `path` (accepts `str` or `os.PathLike`).
    fn save_screenshot(&self, py: Python<'_>, path: &Bound<'_, PyAny>) -> PyResult<()> {
        let os = py.import("os")?;
        let path_str: String = os.call_method1("fspath", (path,))?.extract()?;
        match self.inner.screenshot_png() {
            Some(png) => std::fs::write(&path_str, png).map_err(|e| EngineError::new_err(format!("write failed: {e}"))),
            None => Err(EngineError::new_err(
                "no screenshot to save (pass screenshot=True to fetch())",
            )),
        }
    }

    /// Serialize the page to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PyRuntimeError::new_err(format!("serialize failed: {e}")))
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name = slf.get_type().qualname()?;
        let this = slf.borrow();
        Ok(format!(
            "{name}(url={:?}, title={:?}, html_len={})",
            this.url,
            this.inner.title.as_deref().unwrap_or(""),
            this.inner.html.len()
        ))
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.url.hash(&mut h);
        self.inner.html.len().hash(&mut h);
        h.finish()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.url == other.url && self.inner.html == other.inner.html
    }
}
