//! Input validation for Python-facing entry points.

use std::time::Duration;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyMapping;

pub(crate) const MAX_TIMEOUT_SECS: f64 = 3600.0;
pub(crate) const MAX_SETTLE_SECS: f64 = 60.0;
pub(crate) const MAX_URL_LEN: usize = 8192;
pub(crate) const MAX_JS_LEN: usize = 1_000_000;

pub(crate) fn timeout(t: f64) -> PyResult<Duration> {
    if !t.is_finite() || t <= 0.0 || t > MAX_TIMEOUT_SECS {
        return Err(PyValueError::new_err(format!(
            "timeout must be a positive finite number <= {MAX_TIMEOUT_SECS}s, got {t}"
        )));
    }
    Ok(Duration::from_secs_f64(t))
}

pub(crate) fn settle(s: f64) -> PyResult<Duration> {
    if !(0.0..=MAX_SETTLE_SECS).contains(&s) {
        return Err(PyValueError::new_err(format!(
            "settle must be a non-negative finite number <= {MAX_SETTLE_SECS}s, got {s}"
        )));
    }
    Ok(Duration::from_secs_f64(s))
}

/// Validate the `viewport` kwarg against the shared wire shape and the engine bounds.
pub(crate) fn viewport(v: Option<&Bound<'_, PyMapping>>) -> PyResult<Option<servo_fetch::Viewport>> {
    let Some(v) = v else { return Ok(None) };
    let servo_fetch_types::Viewport { width, height } =
        pythonize::depythonize(v).map_err(|e| PyTypeError::new_err(format!("invalid viewport: {e}")))?;
    servo_fetch::Viewport::new(width, height)
        .map(Some)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

pub(crate) fn url(u: &str) -> PyResult<()> {
    if u.len() > MAX_URL_LEN {
        return Err(PyValueError::new_err(format!(
            "URL length {} exceeds limit {MAX_URL_LEN}",
            u.len()
        )));
    }
    Ok(())
}

pub(crate) fn js(source: &str) -> PyResult<()> {
    if source.len() > MAX_JS_LEN {
        return Err(PyValueError::new_err(format!(
            "javascript source length {} exceeds limit {MAX_JS_LEN}",
            source.len()
        )));
    }
    Ok(())
}
