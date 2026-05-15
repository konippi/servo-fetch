//! Crawl and map pyclasses and helpers.

use pyo3::prelude::*;

/// One page from a crawl — either a successful extraction or an error, plus metadata.
#[pyclass(frozen, module = "servo_fetch._native")]
pub(crate) struct CrawlResult {
    inner: servo_fetch::CrawlResult,
}

impl CrawlResult {
    pub(crate) fn from_core(r: servo_fetch::CrawlResult) -> Self {
        Self { inner: r }
    }
}

#[pymethods]
impl CrawlResult {
    #[getter]
    fn url(&self) -> &str {
        &self.inner.url
    }

    #[getter]
    fn depth(&self) -> usize {
        self.inner.depth
    }

    #[getter]
    fn title(&self) -> Option<&str> {
        self.inner.outcome.as_ref().ok().and_then(|p| p.title.as_deref())
    }

    #[getter]
    fn content(&self) -> Option<&str> {
        self.inner.outcome.as_ref().ok().map(|p| p.content.as_str())
    }

    #[getter]
    fn links_found(&self) -> Option<usize> {
        self.inner.outcome.as_ref().ok().map(|p| p.links_found)
    }

    #[getter]
    fn error(&self) -> Option<&str> {
        self.inner.outcome.as_ref().err().map(|e| e.message.as_str())
    }

    /// `True` if the crawl succeeded for this URL.
    #[getter]
    fn ok(&self) -> bool {
        self.inner.outcome.is_ok()
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name = slf.get_type().qualname()?;
        let this = slf.borrow();
        Ok(match &this.inner.outcome {
            Ok(p) => format!(
                "{name}(url={:?}, depth={}, title={:?})",
                this.inner.url,
                this.inner.depth,
                p.title.as_deref().unwrap_or("")
            ),
            Err(e) => format!(
                "{name}(url={:?}, depth={}, error={:?})",
                this.inner.url, this.inner.depth, e.message
            ),
        })
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.inner.url.hash(&mut h);
        self.inner.depth.hash(&mut h);
        h.finish()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner.url == other.inner.url && self.inner.depth == other.inner.depth
    }
}

/// A URL discovered by [`map`](crate::client::Client::map), optionally with a sitemap `lastmod`.
#[pyclass(frozen, module = "servo_fetch._native")]
pub(crate) struct MappedUrl {
    inner: servo_fetch::MappedUrl,
}

impl MappedUrl {
    pub(crate) fn from_core(u: servo_fetch::MappedUrl) -> Self {
        Self { inner: u }
    }
}

#[pymethods]
impl MappedUrl {
    #[getter]
    fn url(&self) -> &str {
        &self.inner.url
    }

    #[getter]
    fn lastmod(&self) -> Option<&str> {
        self.inner.lastmod.as_deref()
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name = slf.get_type().qualname()?;
        Ok(format!("{name}(url={:?})", slf.borrow().inner.url))
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.inner.url.hash(&mut h);
        h.finish()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner.url == other.inner.url
    }
}

pub(crate) fn strings_from_any(value: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<String>> {
    let Some(v) = value else {
        return Ok(Vec::new());
    };
    if let Ok(s) = v.extract::<String>() {
        return Ok(vec![s]);
    }
    let list: Vec<String> = v.extract()?;
    Ok(list)
}
