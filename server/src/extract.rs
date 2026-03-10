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

/// Extracts the client IP from headers or connection info. Never fails —
/// returns `None` if we genuinely can't figure out where the request came from.
pub struct ClientIp(pub Option<String>);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // x-forwarded-for: first IP in the comma-separated list
        if let Some(forwarded) = parts.headers.get("x-forwarded-for")
            && let Ok(value) = forwarded.to_str()
            && let Some(first) = value.split(',').next()
        {
            let ip = first.trim();
            if !ip.is_empty() {
                return Ok(ClientIp(Some(ip.to_string())));
            }
        }

        // x-real-ip
        if let Some(real_ip) = parts.headers.get("x-real-ip")
            && let Ok(value) = real_ip.to_str()
        {
            let ip = value.trim();
            if !ip.is_empty() {
                return Ok(ClientIp(Some(ip.to_string())));
            }
        }

        // fall back to ConnectInfo from the socket
        if let Some(connect_info) = parts
            .extensions
            .get::<axum::extract::connect_info::ConnectInfo<std::net::SocketAddr>>()
        {
            return Ok(ClientIp(Some(connect_info.0.ip().to_string())));
        }

        Ok(ClientIp(None))
    }
}
