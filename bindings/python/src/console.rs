//! `ConsoleMessage` pyclass mirroring `servo_fetch::ConsoleMessage`.

use pyo3::prelude::*;

/// A single browser console message captured during page load.
#[pyclass(frozen, module = "servo_fetch._native")]
pub(crate) struct ConsoleMessage {
    inner: servo_fetch::ConsoleMessage,
}

impl ConsoleMessage {
    pub(crate) fn from_core(msg: &servo_fetch::ConsoleMessage) -> Self {
        Self { inner: msg.clone() }
    }
}

#[pymethods]
impl ConsoleMessage {
    #[getter]
    fn level(&self) -> &'static str {
        self.inner.level.as_str()
    }

    #[getter]
    fn message(&self) -> &str {
        &self.inner.message
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name = slf.get_type().qualname()?;
        let this = slf.borrow();
        Ok(format!(
            "{name}(level={:?}, message={:?})",
            this.inner.level.as_str(),
            this.inner.message
        ))
    }
}
