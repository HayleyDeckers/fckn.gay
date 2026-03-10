use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub struct AppError {
    /// What the user sees — keep it silly but informative, never leak internals
    message: String,
    status: StatusCode,
    /// The real error chain for traces/logs — never shown to users
    internal: Option<anyhow::Error>,
}

impl AppError {
    /// Create an error with a user-facing message. Most common constructor —
    /// validation failures, user errors, etc.
    pub fn message(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status,
            internal: None,
        }
    }

    /// Chain on internal error details for traces/logs. Use when you have
    /// a real error to attach but don't want to leak it to the user.
    pub fn with_internal(mut self, error: impl Into<anyhow::Error>) -> Self {
        self.internal = Some(error.into());
        self
    }
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status.as_u16();
        if let Some(ref internal) = self.internal {
            tracing::error!("API error ({status}): {} — {internal}", self.message);
        } else {
            tracing::error!("API error ({status}): {}", self.message);
        }

        let body = ErrorResponse {
            error: self.message,
        };

        (self.status, Json(body)).into_response()
    }
}

/// Catch-all: bare `?` on anything that converts to `anyhow::Error` gets a
/// generic user message with the real error stashed as internal.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self {
            message: "something went wrong 💀".to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            internal: Some(err.into()),
        }
    }
}
