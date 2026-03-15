use fckn_gay_logging::{FlattenedFormatter, NullFields, SpanFieldLayer};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

use crate::interfaces::LoggingConfig;

// -- OTel filter --------------------------------------------------------------

/// Per-layer filter for the OTel layer: drops all events (they're already
/// captured by the fmt layer as log lines) and applies an independent level
/// filter so you can e.g. gather traces at debug while only printing logs
/// at info.
#[cfg(feature = "otel")]
struct OtelFilter {
    max_level: tracing::level_filters::LevelFilter,
}

#[cfg(feature = "otel")]
impl<S> tracing_subscriber::layer::Filter<S> for OtelFilter {
    fn enabled(
        &self,
        meta: &tracing::Metadata<'_>,
        _cx: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        *meta.level() <= self.max_level
    }

    fn event_enabled(
        &self,
        _event: &tracing::Event<'_>,
        _cx: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        false
    }
}

// -- Subscriber builder -------------------------------------------------------

pub struct SubscriberBuilder {
    config: LoggingConfig,
    #[cfg(feature = "otel")]
    otel_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    #[cfg(feature = "otel")]
    tracing_level: tracing::level_filters::LevelFilter,
}

impl SubscriberBuilder {
    pub fn new(config: LoggingConfig) -> Self {
        Self {
            config,
            #[cfg(feature = "otel")]
            otel_provider: None,
            #[cfg(feature = "otel")]
            tracing_level: tracing::level_filters::LevelFilter::INFO,
        }
    }

    #[cfg(feature = "otel")]
    pub fn with_otel(
        mut self,
        provider: opentelemetry_sdk::trace::SdkTracerProvider,
        level: tracing::level_filters::LevelFilter,
    ) -> Self {
        self.otel_provider = Some(provider);
        self.tracing_level = level;
        self
    }

    /// Install the global tracing subscriber. Call this exactly once.
    pub fn init(self) {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(FlattenedFormatter::default())
            .fmt_fields(NullFields)
            .with_filter(self.config.level.into_env_filter());

        let registry = tracing_subscriber::registry()
            .with(SpanFieldLayer)
            .with(fmt_layer);

        #[cfg(feature = "otel")]
        {
            use opentelemetry::trace::TracerProvider;

            let otel_layer = self.otel_provider.map(|provider| {
                tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer("fckn-gay-server"))
                    .with_location(false)
                    .with_threads(false)
                    .with_tracked_inactivity(false)
                    .with_filter(OtelFilter { max_level: self.tracing_level })
            });
            registry.with(otel_layer).init();
        }

        #[cfg(not(feature = "otel"))]
        {
            registry.init();
        }
    }
}
