//! Error type for the shared [`crate::tools`] operations.

use servo_fetch_types::ErrorKind;

// MCP disposition and legacy HTTP/RPC error kind are intentionally independent.
#[derive(Debug, Clone, Copy)]
enum ToolErrorClass {
    InvalidInput(ErrorKind),
    Operation(ErrorKind),
    Internal(ErrorKind),
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct ToolError {
    class: ToolErrorClass,
    message: String,
    #[source]
    source: Option<servo_fetch::Error>,
}

impl ToolError {
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            class: ToolErrorClass::Internal(ErrorKind::Internal),
            message: message.into(),
            source: None,
        }
    }

    pub(crate) fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            class: ToolErrorClass::InvalidInput(ErrorKind::InvalidParams),
            message: message.into(),
            source: None,
        }
    }

    pub(crate) fn operation(message: impl Into<String>) -> Self {
        Self {
            class: ToolErrorClass::Operation(ErrorKind::Other),
            message: message.into(),
            source: None,
        }
    }

    pub(crate) fn kind(&self) -> ErrorKind {
        match self.class {
            ToolErrorClass::InvalidInput(kind) | ToolErrorClass::Operation(kind) | ToolErrorClass::Internal(kind) => {
                kind
            }
        }
    }

    pub(crate) fn is_operation(&self) -> bool {
        matches!(self.class, ToolErrorClass::Operation(_))
    }

    pub(crate) fn is_internal(&self) -> bool {
        matches!(self.class, ToolErrorClass::Internal(_))
    }
}

impl From<servo_fetch::Error> for ToolError {
    fn from(err: servo_fetch::Error) -> Self {
        use servo_fetch::Error as E;
        use servo_fetch::extract::ExtractError;

        let class = match &err {
            E::InvalidUrl { .. } => ToolErrorClass::InvalidInput(ErrorKind::InvalidUrl),
            E::AddressNotAllowed { .. } => ToolErrorClass::InvalidInput(ErrorKind::AddressNotAllowed),
            E::Cookies { .. } | E::Schema(_) | E::InvalidGlob(_) | E::InvalidHeader(_) => {
                ToolErrorClass::InvalidInput(ErrorKind::InvalidParams)
            }
            E::Extract(ExtractError::InvalidSelector) => ToolErrorClass::InvalidInput(ErrorKind::Internal),
            E::Timeout { .. } => ToolErrorClass::Operation(ErrorKind::Timeout),
            E::JavaScript { .. } => ToolErrorClass::Operation(ErrorKind::Javascript),
            E::Engine { .. } | E::Screenshot { .. } => ToolErrorClass::Operation(ErrorKind::Engine),
            E::OutputTooLarge { .. }
            | E::SessionCancelled
            | E::SessionAcquireTimeout { .. }
            | E::UnsupportedSessionOperation { .. }
            | E::SessionBrokerFull => ToolErrorClass::Operation(ErrorKind::Other),
            E::WorkerProtocolTimeout { .. } | E::InvalidSessionConfig { .. } | E::WorkerUnavailable { .. } => {
                ToolErrorClass::Internal(ErrorKind::Other)
            }
            _ => ToolErrorClass::Internal(ErrorKind::Internal),
        };
        let message = err.to_string();
        Self {
            class,
            message,
            source: Some(err),
        }
    }
}

pub(crate) type ToolResult<T> = Result<T, ToolError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_selector_is_input_but_formatting_failure_is_internal() {
        let invalid_selector = ToolError::from(servo_fetch::Error::Extract(
            servo_fetch::extract::ExtractError::InvalidSelector,
        ));
        assert_eq!(invalid_selector.kind(), ErrorKind::Internal);
        assert!(!invalid_selector.is_internal());

        let formatting = ToolError::from(servo_fetch::Error::Extract(servo_fetch::extract::ExtractError::Fmt(
            std::fmt::Error,
        )));
        assert_eq!(formatting.kind(), ErrorKind::Internal);
        assert!(formatting.is_internal());
    }

    #[test]
    fn worker_unavailable_is_internal_with_legacy_other_kind() {
        let error = ToolError::from(servo_fetch::Error::WorkerUnavailable {
            source: std::io::Error::other("private worker transport detail").into(),
        });

        assert_eq!(error.kind(), ErrorKind::Other);
        assert!(error.is_internal());
    }

    #[test]
    fn servo_error_remains_in_source_chain() {
        use std::error::Error as _;

        let error = ToolError::from(servo_fetch::Error::WorkerUnavailable {
            source: std::io::Error::other("worker transport failed").into(),
        });
        let source = error.source().expect("ToolError retains its servo-fetch source");

        assert!(source.downcast_ref::<servo_fetch::Error>().is_some());
        assert!(source.source().is_some());
    }
}
