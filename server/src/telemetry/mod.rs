pub mod logging;
pub mod tracing_setup;

#[cfg(feature = "otel")]
pub use tracing_setup::{build_provider, log_deferred_messages};

/// Returns the current OTel trace ID as a hex string, or `None` if tracing
/// is disabled / no active span.
#[cfg(feature = "otel")]
pub fn current_trace_id() -> Option<String> {
    use opentelemetry::trace::TraceContextExt;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let ctx = tracing::Span::current().context();
    let span_ref = ctx.span();
    let trace_id = span_ref.span_context().trace_id();
    if trace_id == opentelemetry::trace::TraceId::INVALID {
        None
    } else {
        Some(format!("{trace_id}"))
    }
}

#[cfg(not(feature = "otel"))]
pub fn current_trace_id() -> Option<String> {
    fckn_gay_logging::get_current_span_field("trace_id")
}
