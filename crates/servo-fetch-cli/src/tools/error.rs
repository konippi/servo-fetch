//! Error type for the shared [`crate::tools`] operations.

use servo_fetch_types::ErrorKind;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct ToolError {
    kind: ErrorKind,
    message: String,
}

impl ToolError {
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: message.into(),
        }
    }

    pub(crate) fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidParams,
            message: message.into(),
        }
    }

    pub(crate) fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl From<servo_fetch::Error> for ToolError {
    fn from(err: servo_fetch::Error) -> Self {
        use servo_fetch::Error as E;
        let kind = match &err {
            E::InvalidUrl { .. } => ErrorKind::InvalidUrl,
            E::AddressNotAllowed { .. } => ErrorKind::AddressNotAllowed,
            E::Cookies { .. } | E::Schema(_) | E::InvalidGlob(_) | E::InvalidHeader(_) => ErrorKind::InvalidParams,
            E::Timeout { .. } => ErrorKind::Timeout,
            E::JavaScript { .. } => ErrorKind::Javascript,
            E::Engine { .. } | E::Screenshot { .. } => ErrorKind::Engine,
            E::Extract(_) | E::Io(_) => ErrorKind::Internal,
            _ => ErrorKind::Other,
        };
        Self {
            kind,
            message: err.to_string(),
        }
    }
}

pub(crate) type ToolResult<T> = Result<T, ToolError>;
