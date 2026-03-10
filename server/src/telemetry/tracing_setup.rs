use std::time::Duration;

/// Generates a request-level span with method and path fields.
/// Status code is recorded after the response via `RecordStatusOnResponse`.
#[derive(Clone, Debug)]
pub struct MakeRequestSpan;

impl<B> tower_http::trace::MakeSpan<B> for MakeRequestSpan {
    fn make_span(&mut self, request: &axum::http::Request<B>) -> tracing::Span {
        tracing::info_span!(
            "request",
            method = %request.method(),
            path = %request.uri().path(),
            http.status_code = tracing::field::Empty,
            ok = tracing::field::Empty,
        )
    }
}

/// Records status code on the request span once the response is ready.
#[derive(Clone, Debug)]
pub struct RecordStatusOnResponse;

impl<B> tower_http::trace::OnResponse<B> for RecordStatusOnResponse {
    fn on_response(
        self,
        response: &axum::http::Response<B>,
        _latency: Duration,
        span: &tracing::Span,
    ) {
        let status = response.status().as_u16();
        span.record("http.status_code", status);
        span.record("ok", status < 400);
    }
}
