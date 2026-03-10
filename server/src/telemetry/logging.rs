use fckn_gay_logging::{FlattenedFormatter, NullFields, SpanFieldLayer};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

use crate::interfaces::LoggingConfig;

pub struct SubscriberBuilder {
    config: LoggingConfig,
}

impl SubscriberBuilder {
    pub fn new(config: LoggingConfig) -> Self {
        Self { config }
    }

    pub fn init(self) {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(FlattenedFormatter::default())
            .fmt_fields(NullFields)
            .with_filter(self.config.level.into_env_filter());

        tracing_subscriber::registry()
            .with(SpanFieldLayer)
            .with(fmt_layer)
            .init();
    }
}
