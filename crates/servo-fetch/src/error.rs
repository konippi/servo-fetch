//! Error types.

use std::error::Error as StdError;
use std::time::Duration;

/// A specialized `Result` type for servo-fetch.
pub type Result<T> = std::result::Result<T, Error>;

/// Boxed error type used as the `source` of variants that wrap arbitrary errors.
pub(crate) type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Errors from servo-fetch operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The URL is malformed or uses a disallowed scheme.
    #[error("invalid URL '{url}': {reason}")]
    InvalidUrl {
        /// The URL that failed validation.
        url: String,
        /// Why the URL is invalid.
        reason: String,
    },

    /// The page did not finish loading within the configured timeout.
    #[error("page load timed out after {}s at {url}", timeout.as_secs())]
    Timeout {
        /// The URL that timed out.
        url: String,
        /// The timeout that was exceeded.
        timeout: Duration,
    },

    /// The URL resolves to a private or reserved address (SSRF protection).
    #[error("address not allowed: {host}")]
    AddressNotAllowed {
        /// The blocked host.
        host: String,
    },

    /// The Servo engine is unavailable or crashed.
    #[error("engine error: {source}")]
    Engine {
        /// URL being processed, if known.
        url: Option<String>,
        /// Source error.
        #[source]
        source: BoxError,
    },

    /// JavaScript evaluation failed.
    #[error("JavaScript evaluation failed: {source}")]
    JavaScript {
        /// URL being processed, if known.
        url: Option<String>,
        /// Source error.
        #[source]
        source: BoxError,
    },

    /// Screenshot capture failed.
    #[error("screenshot capture failed: {source}")]
    Screenshot {
        /// URL being captured, if known.
        url: Option<String>,
        /// Source error.
        #[source]
        source: BoxError,
    },

    /// Content extraction failed.
    #[error(transparent)]
    Extract(#[from] crate::extract::ExtractError),

    /// Schema-based structured extraction failed.
    #[error(transparent)]
    Schema(#[from] crate::schema::SchemaError),

    /// An I/O error occurred.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A glob pattern is invalid.
    #[error(transparent)]
    InvalidGlob(#[from] globset::Error),
}

impl Error {
    /// Construct an [`Error::Engine`] from any error type, preserving source chain.
    pub(crate) fn engine(source: impl Into<BoxError>, url: Option<String>) -> Self {
        Self::Engine {
            url,
            source: source.into(),
        }
    }

    /// Returns `true` if this is a timeout error.
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Returns `true` if this is a network-related error (timeout or address-policy rejection).
    #[must_use]
    pub fn is_network(&self) -> bool {
        matches!(self, Self::Timeout { .. } | Self::AddressNotAllowed { .. })
    }

    /// Returns the URL associated with this error, if any.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::InvalidUrl { url, .. } | Self::Timeout { url, .. } => Some(url),
            Self::Engine { url, .. } | Self::JavaScript { url, .. } | Self::Screenshot { url, .. } => url.as_deref(),
            _ => None,
        }
    }

    /// Returns the host that was rejected, if this is [`Error::AddressNotAllowed`].
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        match self {
            Self::AddressNotAllowed { host } => Some(host),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum UrlError {
    Invalid(String),
    PrivateAddress(String),
}

pub(crate) fn map_url_error(url: &str, e: UrlError) -> Error {
    match e {
        UrlError::PrivateAddress(host) => Error::AddressNotAllowed { host },
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
    fn assert_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Error>();
    }

    #[test]
    fn timeout_predicates() {
        let err = Error::Timeout {
            url: "https://example.com".into(),
            timeout: Duration::from_secs(30),
        };
        assert!(err.is_timeout());
        assert!(err.is_network());
        assert_eq!(err.url(), Some("https://example.com"));
        assert_eq!(err.host(), None);
    }

    #[test]
    fn address_not_allowed_predicates() {
        let err = Error::AddressNotAllowed {
            host: "127.0.0.1".into(),
        };
        assert!(!err.is_timeout());
        assert!(err.is_network());
        assert_eq!(err.url(), None);
        assert_eq!(err.host(), Some("127.0.0.1"));
    }

    #[test]
    fn invalid_url_carries_url() {
        let err = Error::InvalidUrl {
            url: "bad://url".into(),
            reason: "scheme not allowed".into(),
        };
        assert!(!err.is_network());
        assert_eq!(err.url(), Some("bad://url"));
        assert_eq!(err.host(), None);
    }

    #[test]
    fn engine_helper_preserves_source_chain() {
        let inner = std::io::Error::other("disk full");
        let err = Error::engine(inner, Some("https://example.com".into()));
        assert_eq!(err.url(), Some("https://example.com"));
        assert!(err.source().is_some());
        assert_eq!(err.to_string(), "engine error: disk full");
    }

    #[test]
    fn engine_without_url_returns_none() {
        let err = Error::engine(std::io::Error::other("crash"), None);
        assert!(err.url().is_none());
    }
}
