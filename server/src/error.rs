use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let mut body = format!("{}", self.0);
        let mut source = self.0.source();
        while let Some(error) = source {
            body.push_str("\nCaused by: ");
            body.push_str(&error.to_string());
            source = error.source();
        }
        (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
