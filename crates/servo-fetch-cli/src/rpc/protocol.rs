//! JSON-RPC 2.0 envelope types for the stdio RPC server.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use servo_fetch_types::ErrorKind;

/// JSON-RPC request id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum RequestId {
    Number(i64),
    String(String),
}

/// Inbound request (has `id`) or notification (no `id`).
#[derive(Debug, Deserialize)]
pub(crate) struct Incoming {
    #[serde(default)]
    pub id: Option<RequestId>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Params of the `$/cancelRequest` notification.
#[derive(Debug, Deserialize)]
pub(crate) struct CancelParams {
    pub id: RequestId,
}

/// JSON-RPC error object.
/// Structured payload for JSON-RPC `error.data`.
#[derive(Debug, Serialize)]
pub(crate) struct ErrorData {
    pub kind: ErrorKind,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResponseError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ErrorData>,
}

impl ResponseError {
    pub(crate) fn new(code: i32, kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(ErrorData { kind }),
        }
    }

    pub(crate) fn method_not_found(method: &str) -> Self {
        Self::new(
            code::METHOD_NOT_FOUND,
            ErrorKind::MethodNotFound,
            format!("method not found: {method}"),
        )
    }

    pub(crate) fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(code::INVALID_PARAMS, ErrorKind::InvalidParams, message)
    }

    pub(crate) fn cancelled() -> Self {
        Self::new(
            code::REQUEST_CANCELLED,
            ErrorKind::RequestCancelled,
            "request cancelled",
        )
    }

    pub(crate) fn duplicate_id() -> Self {
        Self::new(
            code::INVALID_REQUEST,
            ErrorKind::DuplicateRequest,
            "duplicate in-flight request id",
        )
    }
}

/// Wire protocol version reported by `initialize`.
pub(crate) const PROTOCOL_VERSION: u32 = 1;

/// Standard JSON-RPC error codes plus LSP's request-cancelled code.
pub(crate) mod code {
    pub(crate) const PARSE_ERROR: i32 = -32700;
    pub(crate) const INVALID_REQUEST: i32 = -32600;
    pub(crate) const METHOD_NOT_FOUND: i32 = -32601;
    pub(crate) const INVALID_PARAMS: i32 = -32602;
    pub(crate) const INTERNAL_ERROR: i32 = -32603;
    pub(crate) const FETCH_ERROR: i32 = -32000;
    pub(crate) const REQUEST_CANCELLED: i32 = -32800;
}

#[derive(Debug, Serialize)]
pub(crate) struct Response {
    pub jsonrpc: &'static str,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

impl Response {
    pub(crate) fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub(crate) fn failure(id: RequestId, error: ResponseError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// Server-to-client notification (e.g. `$/progress`).
#[derive(Debug, Serialize)]
pub(crate) struct Notification {
    pub jsonrpc: &'static str,
    pub method: &'static str,
    pub params: Value,
}

impl Notification {
    pub(crate) fn new(method: &'static str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            method,
            params,
        }
    }
}
