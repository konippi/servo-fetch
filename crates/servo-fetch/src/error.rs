//! Error types.

use std::time::Duration;

/// A specialized `Result` type for servo-fetch.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors from servo-fetch operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The URL is malformed or uses a disallowed scheme.
    #[error("{reason}")]
    InvalidUrl {
        /// The URL that failed validation.
        url: String,
        /// Why the URL is invalid.
        reason: String,
    },

    /// The page did not finish loading within the configured timeout.
    #[error("page load timed out after {}s", timeout.as_secs())]
    Timeout {
        /// The URL that timed out.
        url: String,
        /// The timeout that was exceeded.
        timeout: Duration,
    },

    /// The URL resolves to a private or reserved address (SSRF protection).
    #[error("address not allowed: {0}")]
    AddressNotAllowed(String),

    /// The Servo engine is unavailable or crashed.
    #[error("engine error: {0}")]
    Engine(String),

    /// JavaScript evaluation failed.
    #[error("JavaScript evaluation failed: {0}")]
    JavaScript(String),

    /// Screenshot capture failed.
    #[error("screenshot capture failed: {0}")]
    Screenshot(String),

    /// Content extraction failed.
    #[error(transparent)]
    Extract(#[from] crate::extract::ExtractError),

    /// An I/O error occurred.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A glob pattern is invalid.
    #[error("invalid glob pattern: {0}")]
    InvalidGlob(#[from] globset::Error),
}

impl Error {
    /// Returns `true` if this is a timeout error.
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Returns `true` if this is a network-related error.
    #[must_use]
    pub fn is_network(&self) -> bool {
        matches!(self, Self::Timeout { .. } | Self::AddressNotAllowed(_))
    }

    /// Returns the URL associated with this error, if any.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::InvalidUrl { url, .. } | Self::Timeout { url, .. } | Self::AddressNotAllowed(url) => Some(url),
            _ => None,
        }
    }
}

#[allow(clippy::used_underscore_items)]
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<Error>();
    }
};

/// Why a URL was rejected by [`validate_url`].
#[derive(Debug)]
pub(crate) enum UrlError {
    /// Malformed URL or disallowed scheme.
    Invalid(String),
    /// Host resolves to a private or reserved address.
    PrivateAddress(String),
}

impl std::fmt::Display for UrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => f.write_str(reason),
            Self::PrivateAddress(host) => {
                write!(f, "access to private/local addresses is not allowed: {host}")
            }
        }
    }
}

pub(crate) fn map_url_error(url: &str, e: UrlError) -> Error {
    match e {
        UrlError::PrivateAddress(host) => Error::AddressNotAllowed(host),
        UrlError::Invalid(reason) => Error::InvalidUrl {
            url: url.into(),
            reason,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_is_timeout() {
        let err = Error::Timeout {
            url: "https://example.com".into(),
            timeout: Duration::from_secs(30),
        };
        assert!(err.is_timeout());
        assert!(err.is_network());
        assert_eq!(err.url(), Some("https://example.com"));
    }

    #[test]
    fn address_not_allowed_is_network() {
        let err = Error::AddressNotAllowed("127.0.0.1".into());
        assert!(!err.is_timeout());
        assert!(err.is_network());
        assert_eq!(err.url(), Some("127.0.0.1"));
    }

    #[test]
    fn invalid_url_has_url() {
        let err = Error::InvalidUrl {
            url: "bad://url".into(),
            reason: "scheme not allowed".into(),
        };
        assert!(!err.is_timeout());
        assert!(!err.is_network());
        assert_eq!(err.url(), Some("bad://url"));
    }

    #[test]
    fn engine_error_has_no_url() {
        let err = Error::Engine("crashed".into());
        assert!(!err.is_timeout());
        assert!(err.url().is_none());
    }
}
