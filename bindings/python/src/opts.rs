//! Build [`servo_fetch::FetchOptions`] from Python kwargs.

use std::collections::HashMap;
use std::path::PathBuf;

use pyo3::prelude::*;

use crate::schema::Schema;
use crate::validate;

pub(crate) struct BuildOpts<'py> {
    pub url: String,
    pub timeout: Option<f64>,
    pub settle: Option<f64>,
    pub user_agent: Option<String>,
    pub screenshot: bool,
    pub javascript: Option<String>,
    pub schema: Option<Bound<'py, Schema>>,
    pub cookies_file: Option<PathBuf>,
    pub headers: Option<HashMap<String, String>>,
}

pub(crate) struct Prepared {
    pub opts: servo_fetch::FetchOptions,
    pub url: String,
    pub screenshot_requested: bool,
    pub js_requested: bool,
}

pub(crate) fn prepare(args: BuildOpts<'_>) -> PyResult<Prepared> {
    validate::url(&args.url)?;
    if let Some(ref j) = args.javascript {
        validate::js(j)?;
    }
    let timeout = args.timeout.map(validate::timeout).transpose()?;
    let settle = args.settle.map(validate::settle).transpose()?;

    let schema_inner = args.schema.as_ref().map(|s| s.borrow().inner().clone());

    let screenshot_requested = args.screenshot;
    let js_requested = args.javascript.is_some();

    let mut opts = match (args.screenshot, args.javascript.as_deref()) {
        (true, _) => servo_fetch::FetchOptions::screenshot(&args.url, true),
        (false, Some(expr)) => servo_fetch::FetchOptions::javascript(&args.url, expr.to_string()),
        (false, None) => servo_fetch::FetchOptions::new(&args.url),
    };
    if let Some(t) = timeout {
        opts = opts.timeout(t);
    }
    if let Some(s) = settle {
        opts = opts.settle(s);
    }
    if let Some(ua) = args.user_agent {
        opts = opts.user_agent(ua);
    }
    if let Some(schema) = schema_inner {
        opts = opts.schema(schema);
    }
    if let Some(path) = &args.cookies_file {
        let cookies = servo_fetch::load_cookies(path).map_err(crate::errors::map_error)?;
        if !cookies.is_empty() {
            opts = opts.cookies(cookies);
        }
    }
    if let Some(map) = &args.headers {
        opts = opts.headers(servo_fetch::headers::from_pairs(map).map_err(crate::errors::map_error)?);
    }

    Ok(Prepared {
        opts,
        url: args.url,
        screenshot_requested,
        js_requested,
    })
}
