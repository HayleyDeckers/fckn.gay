use crate::interfaces::LoggingConfig;

pub fn init(config: LoggingConfig) {
    // .init() auto-installs the log→tracing bridge when env-filter is enabled
    tracing_subscriber::fmt()
        .with_env_filter(config.level.into_env_filter())
        .init();
}
