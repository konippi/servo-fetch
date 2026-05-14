//! Exception hierarchy and error mapping from `servo_fetch::Error`.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

create_exception!(
    servo_fetch,
    ServoFetchError,
    PyException,
    "Base class for all servo-fetch exceptions."
);
create_exception!(
    servo_fetch,
    InvalidUrlError,
    ServoFetchError,
    "The supplied URL is malformed or uses a disallowed scheme."
);
create_exception!(
    servo_fetch,
    FetchTimeoutError,
    ServoFetchError,
    "The page did not finish loading within the configured timeout."
);
create_exception!(
    servo_fetch,
    NetworkError,
    ServoFetchError,
    "Network or SSRF-policy failure (e.g. private address blocked)."
);
create_exception!(
    servo_fetch,
    EngineError,
    ServoFetchError,
    "The Servo engine is unavailable or crashed."
);
create_exception!(
    servo_fetch,
    SchemaError,
    ServoFetchError,
    "Schema parse, validation, or I/O failure."
);

pub(crate) fn map_error(err: servo_fetch::Error) -> PyErr {
    match &err {
        servo_fetch::Error::Timeout { url, timeout } => FetchTimeoutError::new_err(format!(
            "page load timed out after {}s at {}",
            timeout.as_secs(),
            redact_url(url)
        )),
        servo_fetch::Error::InvalidUrl { url, reason } => {
            InvalidUrlError::new_err(format!("invalid URL '{}': {reason}", redact_url(url)))
        }
        servo_fetch::Error::AddressNotAllowed(host) => NetworkError::new_err(format!("address not allowed: {host}")),
        servo_fetch::Error::Engine(msg) => EngineError::new_err(msg.clone()),
        servo_fetch::Error::Schema(e) => SchemaError::new_err(e.to_string()),
        _ => ServoFetchError::new_err(err.to_string()),
    }
}

fn redact_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut u) if u.password().is_some_and(|p| !p.is_empty()) => {
            let _ = u.set_password(Some("***"));
            u.to_string()
        }
        _ => url.to_string(),
    }
}

pub(crate) fn register(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("ServoFetchError", py.get_type::<ServoFetchError>())?;
    m.add("InvalidUrlError", py.get_type::<InvalidUrlError>())?;
    m.add("FetchTimeoutError", py.get_type::<FetchTimeoutError>())?;
    m.add("NetworkError", py.get_type::<NetworkError>())?;
    m.add("EngineError", py.get_type::<EngineError>())?;
    m.add("SchemaError", py.get_type::<SchemaError>())?;
    Ok(())
}
