//! Thin wrappers around axum's built-in extractors that reject through
//! `AppError` instead of plain-text responses. The real rejection is
//! attached via `.with_internal()` so the full `source()` chain
//! (e.g. `JsonRejection` -> `serde_json::Error`) ends up in logs.

use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    http,
    response::{IntoResponse, Response},
};

use crate::error::AppError;

/// Generates a newtype wrapper around an axum extractor that rejects through
/// `AppError`. The inner value is unwrapped so `Json(val)` / `Query(val)` /
/// etc. work in handler signatures without double-nesting.
macro_rules! app_error_extractor {
    // Body-consuming extractors (Json, Form, …)
    (from_request, $Name:ident, $($axum:ident)::+, $status:expr, $msg:expr) => {
        pub struct $Name<T>(pub T);

        impl<S, T> FromRequest<S> for $Name<T>
        where
            $($axum)::+<T>: FromRequest<S>,
            <$($axum)::+<T> as FromRequest<S>>::Rejection: Into<anyhow::Error>,
            S: Send + Sync,
        {
            type Rejection = AppError;

            async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
                match <$($axum)::+<T>>::from_request(req, state).await {
                    Ok(v) => Ok($Name(v.0)),
                    Err(rejection) => {
                        Err(AppError::message($status, $msg).with_internal(rejection))
                    }
                }
            }
        }
    };

    // Non-body extractors (Query, Path, …)
    (from_request_parts, $Name:ident, $($axum:ident)::+, $status:expr, $msg:expr) => {
        pub struct $Name<T>(pub T);

        impl<S, T> FromRequestParts<S> for $Name<T>
        where
            $($axum)::+<T>: FromRequestParts<S>,
            <$($axum)::+<T> as FromRequestParts<S>>::Rejection: Into<anyhow::Error>,
            S: Send + Sync,
        {
            type Rejection = AppError;

            async fn from_request_parts(
                parts: &mut http::request::Parts,
                state: &S,
            ) -> Result<Self, Self::Rejection> {
                match <$($axum)::+<T>>::from_request_parts(parts, state).await {
                    Ok(v) => Ok($Name(v.0)),
                    Err(rejection) => {
                        Err(AppError::message($status, $msg).with_internal(rejection))
                    }
                }
            }
        }
    };
}

app_error_extractor!(
    from_request,
    Json,
    axum::Json,
    http::StatusCode::BAD_REQUEST,
    "failed to parse JSON body"
);

app_error_extractor!(
    from_request,
    Form,
    axum::extract::Form,
    http::StatusCode::BAD_REQUEST,
    "failed to parse form body"
);

app_error_extractor!(
    from_request_parts,
    Query,
    axum::extract::Query,
    http::StatusCode::BAD_REQUEST,
    "failed to parse query string"
);

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}
