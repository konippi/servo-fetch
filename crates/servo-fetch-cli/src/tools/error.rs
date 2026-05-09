//! Error type for the shared [`crate::tools`] operations.

#[derive(Debug, thiserror::Error)]
pub(crate) enum ToolError {
    /// Caller-supplied input is malformed or violates a policy (bad URL, scheme, SSRF block, over-size limits).
    #[error("{0}")]
    InvalidInput(String),

    /// Upstream page render or network operation failed.
    #[error("{0}")]
    Fetch(String),

    /// Unexpected server-side failure (semaphore poisoning, task join panic, etc.).
    #[error("{0}")]
    Internal(String),
}

impl ToolError {
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    pub(crate) fn fetch(msg: impl Into<String>) -> Self {
        Self::Fetch(msg.into())
    }

    pub(crate) fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

pub(crate) type ToolResult<T> = Result<T, ToolError>;
