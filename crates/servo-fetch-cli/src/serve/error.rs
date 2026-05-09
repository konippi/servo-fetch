//! HTTP API error type with a consistent JSON response shape.

use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::tools::ToolError;

#[derive(Debug)]
pub(super) struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub(super) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self {
            status: rejection.status(),
            message: rejection.body_text(),
        }
    }
}

impl From<ToolError> for ApiError {
    fn from(err: ToolError) -> Self {
        let message = err.to_string();
        let status = match err {
            ToolError::InvalidInput(_) => StatusCode::BAD_REQUEST,
            ToolError::Fetch(_) => StatusCode::BAD_GATEWAY,
            ToolError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self { status, message }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.message });
        (self.status, axum::Json(body)).into_response()
    }
}
