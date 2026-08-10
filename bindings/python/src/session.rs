//! `Session` pyclass: process-isolated browser session.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use pyo3::prelude::*;

use crate::errors::map_error;
use crate::opts::{BuildOpts, prepare};
use crate::page::Page;
use crate::validate;

/// Route isolated workers through `python -m servo_fetch._worker` unless overridden.
fn configure_worker_command(py: Python<'_>) -> PyResult<()> {
    static CONFIGURED: OnceLock<()> = OnceLock::new();
    if CONFIGURED.get().is_some() {
        return Ok(());
    }
    let program = match std::env::var_os("SERVO_FETCH_WORKER") {
        Some(program) => PathBuf::from(program),
        None => py.import("sys")?.getattr("executable")?.extract::<PathBuf>()?,
    };
    let command = servo_fetch::WorkerCommand::new(program).args(["-m", "servo_fetch._worker"]);
    let _ = servo_fetch::set_default_worker_command(command);
    let _ = CONFIGURED.set(());
    Ok(())
}

/// Run the isolated worker protocol on stdio.
#[pyfunction]
pub(crate) fn run_worker_stdio(py: Python<'_>) -> PyResult<()> {
    py.detach(servo_fetch::run_worker_stdio).map_err(map_error)
}

/// An isolated browser session backed by a one-use worker process.
#[pyclass(module = "servo_fetch._native")]
pub(crate) struct Session {
    inner: Mutex<Option<servo_fetch::BrowserSession>>,
}

#[pymethods]
impl Session {
    #[new]
    #[pyo3(signature = (*, user_agent=None, cookies_file=None, cookies_url=None))]
    fn new(
        py: Python<'_>,
        user_agent: Option<String>,
        cookies_file: Option<PathBuf>,
        cookies_url: Option<String>,
    ) -> PyResult<Self> {
        configure_worker_command(py)?;
        let mut config = servo_fetch::BrowserSessionConfig::new();
        if let Some(user_agent) = user_agent {
            config = config.user_agent(user_agent);
        }
        match (cookies_file, cookies_url) {
            (Some(path), Some(url)) => {
                validate::url(&url)?;
                let cookies = servo_fetch::load_cookies(&path).map_err(map_error)?;
                config = config.cookies(url, cookies);
            }
            (None, None) => {}
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "cookies_file and cookies_url must be provided together",
                ));
            }
        }
        let session = py
            .detach(|| servo_fetch::BrowserSession::new_blocking(config))
            .map_err(map_error)?;
        Ok(Self {
            inner: Mutex::new(Some(session)),
        })
    }

    /// Fetch a URL inside this session, preserving cookies and storage across calls.
    #[pyo3(signature = (url, *, timeout=None, settle=None, screenshot=false, javascript=None, headers=None))]
    #[allow(clippy::too_many_arguments)]
    fn fetch(
        &self,
        py: Python<'_>,
        url: String,
        timeout: Option<f64>,
        settle: Option<f64>,
        screenshot: bool,
        javascript: Option<String>,
        headers: Option<HashMap<String, String>>,
    ) -> PyResult<Page> {
        let prepared = prepare(BuildOpts {
            url,
            timeout,
            settle,
            user_agent: None,
            screenshot,
            javascript,
            schema: None,
            cookies_file: None,
            headers,
        })?;
        let servo_page = py
            .detach(|| {
                let mut guard = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let session = guard.as_mut().ok_or_else(|| servo_fetch::Error::WorkerUnavailable {
                    source: "browser session is closed".into(),
                })?;
                session.fetch_blocking(&prepared.opts)
            })
            .map_err(map_error)?;
        Ok(Page::new(
            servo_page,
            prepared.url,
            prepared.screenshot_requested,
            prepared.js_requested,
        ))
    }

    /// Close the session, tearing down its worker process and storage.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        // Lock acquisition may wait behind a long fetch, so it must not hold the GIL.
        py.detach(|| {
            let session = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            match session {
                Some(session) => session.close_blocking(),
                None => Ok(()),
            }
        })
        .map_err(map_error)
    }

    /// Whether `close` has been called; an in-flight fetch still counts as open.
    #[getter]
    fn is_closed(&self) -> bool {
        match self.inner.try_lock() {
            Ok(guard) => guard.is_none(),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner().is_none(),
            Err(std::sync::TryLockError::WouldBlock) => false,
        }
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, pyo3::types::PyTuple>) -> PyResult<()> {
        self.close(py)
    }
}
