//! HTTP API error type with a consistent JSON response shape.

use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use servo_fetch_types::ErrorKind;

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
        let status = match err.kind() {
            ErrorKind::InvalidUrl | ErrorKind::AddressNotAllowed | ErrorKind::InvalidParams => StatusCode::BAD_REQUEST,
            ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_GATEWAY,
        };
        Self { status, message }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = servo_fetch_types::ErrorEnvelope { error: self.message };
        (self.status, axum::Json(body)).into_response()
    }
}
