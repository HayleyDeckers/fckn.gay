use serde::Deserialize;

#[derive(Deserialize, Default, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TracingBackend {
    #[default]
    Disabled,
    Stdout,
    Otlp,
}

/// Parse W3C traceparent header, return the 32-hex-char trace ID.
/// Format: `VV-TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT-PPPPPPPPPPPPPPPP-FF`
fn parse_traceparent_trace_id(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.len() < 55 {
        return None;
    }
    if bytes[2] != b'-' || bytes[35] != b'-' || bytes[52] != b'-' {
        return None;
    }
    // W3C spec: version ff is reserved and must be treated as invalid
    if &value[0..2] == "ff" {
        return None;
    }
    let trace_id = &value[3..35];
    if !trace_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if trace_id == "00000000000000000000000000000000" {
        return None;
    }
    Some(trace_id)
}

/// Generate a random hex trace ID of the given length (capped at 32 chars).
fn generate_random_trace_id(chars: usize) -> String {
    let chars = chars.min(32);
    let bytes_needed = (chars + 1) / 2;
    let mut buf = [0u8; 16];
    let _ = getrandom::fill(&mut buf[..bytes_needed]);
    let mut s = String::with_capacity(chars);
    for b in &buf[..bytes_needed] {
        use std::fmt::Write;
        write!(s, "{b:02x}").ok();
    }
    s.truncate(chars);
    s
}

// -- Always-available request span + response recorder -------------------------

/// Creates the root HTTP span for each request — always on, even without OTel.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "otel", allow(dead_code))]
pub struct MakeRequestSpan {
    pub trust_incoming_spans: bool,
    pub trace_id_chars: usize,
}

impl<B> tower_http::trace::MakeSpan<B> for MakeRequestSpan {
    fn make_span(&mut self, request: &axum::http::Request<B>) -> tracing::Span {
        let span = tracing::info_span!(
            "request",
            method = %request.method(),
            path = %request.uri().path(),
            trace_id = tracing::field::Empty,
            http.status_code = tracing::field::Empty,
            ok = tracing::field::Empty,
        );

        let chars = self.trace_id_chars;

        // If we trust the caller, try to extract trace ID from traceparent header
        let trace_id = if self.trust_incoming_spans {
            request
                .headers()
                .get("traceparent")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_traceparent_trace_id)
                .map(|tid| tid[..chars.min(32)].to_owned())
        } else {
            None
        }
        .unwrap_or_else(|| generate_random_trace_id(chars));

        span.record("trace_id", &trace_id);
        span
    }
}

/// Records `http.status_code` and `ok` on the span when the response is ready.
#[derive(Clone, Debug)]
pub struct RecordStatusOnResponse;

impl<B> tower_http::trace::OnResponse<B> for RecordStatusOnResponse {
    fn on_response(
        self,
        response: &axum::http::Response<B>,
        _latency: std::time::Duration,
        span: &tracing::Span,
    ) {
        let status = response.status().as_u16();
        span.record("http.status_code", status);
        span.record("ok", status < 400);
    }
}

// -- OTel-only: W3C context extraction + provider lifecycle --------------------

#[cfg(feature = "otel")]
mod otel {
    use opentelemetry::propagation::Extractor;
    use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};

    use super::{TracingBackend, generate_random_trace_id};
    use crate::interfaces::TracingConfig;

    /// Wraps a SpanProcessor to strip `trace_id` from span attributes before
    /// export. We record `trace_id` on request spans for the log formatter,
    /// but OTel already carries the real trace ID in the span context — no
    /// need to duplicate it as an attribute and clutter up Jaeger/Honeycomb/etc.
    #[derive(Debug)]
    struct StripTraceIdProcessor<P>(P);

    impl<P: opentelemetry_sdk::trace::SpanProcessor> opentelemetry_sdk::trace::SpanProcessor
        for StripTraceIdProcessor<P>
    {
        fn on_start(
            &self,
            span: &mut opentelemetry_sdk::trace::Span,
            cx: &opentelemetry::Context,
        ) {
            self.0.on_start(span, cx);
        }

        fn on_end(&self, mut span: opentelemetry_sdk::trace::SpanData) {
            span.attributes.retain(|kv| kv.key.as_str() != "trace_id");
            self.0.on_end(span);
        }

        fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
            self.0.force_flush()
        }

        fn shutdown(&self) -> opentelemetry_sdk::error::OTelSdkResult {
            self.0.shutdown()
        }
    }

    /// Messages collected during provider setup (before the tracing subscriber
    /// is initialized), logged by main() after init.
    pub struct ProviderOutput {
        pub provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
        pub messages: Vec<(tracing::Level, String)>,
    }

    /// Builds the OTel tracer provider based on config. Returns `None` if tracing
    /// is disabled, otherwise sets the global provider and returns it so we can
    /// build a `tracing_opentelemetry` layer from it.
    ///
    /// Log messages are deferred — the subscriber isn't installed yet at this
    /// point, so we return them for main() to emit after init.
    pub fn build_provider(config: &TracingConfig) -> ProviderOutput {
        let mut messages: Vec<(tracing::Level, String)> = Vec::new();

        match config.provider {
            TracingBackend::Disabled => {
                messages.push((tracing::Level::INFO, "distributed tracing disabled".into()));
                ProviderOutput {
                    provider: None,
                    messages,
                }
            }
            TracingBackend::Stdout => {
                if config.trust_incoming_spans {
                    install_propagator(&mut messages);
                }
                let exporter = opentelemetry_stdout::SpanExporter::default();
                let processor = opentelemetry_sdk::trace::SimpleSpanProcessor::new(exporter);

                let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                    .with_span_processor(StripTraceIdProcessor(processor))
                    .with_resource(
                        opentelemetry_sdk::Resource::builder()
                            .with_service_name(config.otlp.service_name.clone())
                            .build(),
                    )
                    .build();

                opentelemetry::global::set_tracer_provider(provider.clone());
                messages.push((
                    tracing::Level::INFO,
                    "OTel tracing enabled (stdout — for debugging only!)".into(),
                ));
                ProviderOutput {
                    provider: Some(provider),
                    messages,
                }
            }
            TracingBackend::Otlp => {
                if config.trust_incoming_spans {
                    install_propagator(&mut messages);
                }

                let mut metadata = tonic::metadata::MetadataMap::new();
                for (key, secret) in &config.otlp.headers {
                    let Ok(k) =
                        key.parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>()
                    else {
                        messages.push((
                            tracing::Level::WARN,
                            format!("invalid OTLP header name: {key}"),
                        ));
                        continue;
                    };
                    let Ok(v) = secret
                        .expose()
                        .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
                    else {
                        messages.push((
                            tracing::Level::WARN,
                            format!("invalid OTLP header value for {key}"),
                        ));
                        continue;
                    };
                    metadata.insert(k, v);
                }

                let mut tonic_builder = opentelemetry_otlp::SpanExporter::builder()
                    .with_tonic()
                    .with_endpoint(&config.otlp.endpoint);
                if !metadata.is_empty() {
                    tonic_builder = tonic_builder.with_metadata(metadata);
                }
                let exporter = tonic_builder
                    .build()
                    .expect("failed to build OTLP exporter");
                let processor =
                    opentelemetry_sdk::trace::BatchSpanProcessor::builder(exporter).build();

                let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                    .with_span_processor(StripTraceIdProcessor(processor))
                    .with_resource(
                        opentelemetry_sdk::Resource::builder()
                            .with_service_name(config.otlp.service_name.clone())
                            .build(),
                    )
                    .build();

                opentelemetry::global::set_tracer_provider(provider.clone());
                messages.push((
                    tracing::Level::INFO,
                    format!("OTel tracing enabled (endpoint: {})", config.otlp.endpoint),
                ));
                ProviderOutput {
                    provider: Some(provider),
                    messages,
                }
            }
        }
    }

    fn install_propagator(messages: &mut Vec<(tracing::Level, String)>) {
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );
        messages.push((
            tracing::Level::INFO,
            "W3C trace context propagation enabled (trust_incoming_spans = true)".into(),
        ));
    }

    /// Wrapper so we can impl `Extractor` on a borrowed `HeaderMap` without
    /// pulling in `opentelemetry-http` as a direct dep.
    struct HeaderMapExtractor<'a>(&'a axum::http::HeaderMap);

    impl Extractor for HeaderMapExtractor<'_> {
        fn get(&self, key: &str) -> Option<&str> {
            self.0.get(key).and_then(|v| v.to_str().ok())
        }

        fn keys(&self) -> Vec<&str> {
            self.0.keys().map(|k| k.as_str()).collect()
        }
    }

    /// Wraps `MakeRequestSpan` with W3C trace context extraction from headers.
    #[derive(Clone, Debug)]
    pub struct OtelMakeSpan {
        pub trust_incoming_spans: bool,
        pub trace_id_chars: usize,
    }

    impl<B> tower_http::trace::MakeSpan<B> for OtelMakeSpan {
        fn make_span(&mut self, request: &axum::http::Request<B>) -> tracing::Span {
            // Only attach incoming context when we trust the caller
            let _guard = self.trust_incoming_spans.then(|| {
                let parent_cx = opentelemetry::global::get_text_map_propagator(|propagator| {
                    propagator.extract(&HeaderMapExtractor(request.headers()))
                });
                parent_cx.attach()
            });

            let span = tracing::info_span!(
                "request",
                method = %request.method(),
                path = %request.uri().path(),
                trace_id = tracing::field::Empty,
                http.status_code = tracing::field::Empty,
                ok = tracing::field::Empty,
            );

            // OTel layer already assigned a trace ID during on_new_span
            use opentelemetry::trace::TraceContextExt;
            use tracing_opentelemetry::OpenTelemetrySpanExt;
            let ctx = span.context();
            let otel_tid = ctx.span().span_context().trace_id();
            let tid_str = format!("{otel_tid}");
            let chars = self.trace_id_chars.min(tid_str.len());
            span.record("trace_id", &tid_str[..chars]);

            // If the OTel trace ID is invalid (no OTel layer active), fall back
            // to a random ID so we still get log correlation
            if otel_tid == opentelemetry::trace::TraceId::INVALID {
                span.record("trace_id", generate_random_trace_id(self.trace_id_chars));
            }

            span
        }
    }

}

#[cfg(feature = "otel")]
pub use otel::*;

/// Emit deferred log messages from OTel provider setup.
/// Called from main() after the tracing subscriber is initialized.
#[cfg(feature = "otel")]
pub fn log_deferred_messages(messages: Vec<(tracing::Level, String)>) {
    for (level, msg) in messages {
        match level {
            tracing::Level::ERROR => tracing::error!("{msg}"),
            tracing::Level::WARN => tracing::warn!("{msg}"),
            tracing::Level::INFO => tracing::info!("{msg}"),
            tracing::Level::DEBUG => tracing::debug!("{msg}"),
            tracing::Level::TRACE => tracing::trace!("{msg}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_traceparent() {
        let tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        assert_eq!(
            parse_traceparent_trace_id(tp),
            Some("4bf92f3577b34da6a3ce929d0e0e4736")
        );
    }

    #[test]
    fn all_zeros_invalid() {
        let tp = "00-00000000000000000000000000000000-00f067aa0ba902b7-01";
        assert_eq!(parse_traceparent_trace_id(tp), None);
    }

    #[test]
    fn too_short() {
        assert_eq!(parse_traceparent_trace_id("00-abc-def-01"), None);
    }

    #[test]
    fn non_hex_chars() {
        let tp = "00-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz-00f067aa0ba902b7-01";
        assert_eq!(parse_traceparent_trace_id(tp), None);
    }

    #[test]
    fn version_ff_invalid() {
        let tp = "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        assert_eq!(parse_traceparent_trace_id(tp), None);
    }

    #[test]
    fn random_trace_id_length() {
        assert_eq!(generate_random_trace_id(8).len(), 8);
        assert_eq!(generate_random_trace_id(16).len(), 16);
        assert_eq!(generate_random_trace_id(32).len(), 32);
        // capped at 32
        assert_eq!(generate_random_trace_id(64).len(), 32);
    }
}
