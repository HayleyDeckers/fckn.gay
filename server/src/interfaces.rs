use std::{collections::HashMap, fmt::Display, path::Path, sync::Arc};

use anyhow::{Context, Result};
use axum::extract::FromRef;
use fckn_gay_dns::{Dns, Interface as DnsInterface};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseIntferface};
use serde::{Deserialize, Deserializer};
use tracing_subscriber::EnvFilter;

use crate::{
    auth_cache::{AuthenticationCache, PasswordResetCache},
    captcha::TurnstileVerifier,
};

/// The server's bind address, made available to handlers that need to construct
/// self-referential URLs (e.g. password reset links).
#[derive(Clone)]
pub struct ServerAddress(Arc<String>);

impl std::fmt::Display for ServerAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Rate limiting configuration for both auth (IP-based) and API (user-based) routes.
/// All values are configurable through the server config file.
#[derive(Clone, Debug, Deserialize)]
pub struct RateLimitConfig {
    /// Max burst of requests for auth routes (login, signup, etc) before limiting kicks in
    #[serde(default = "RateLimitConfig::default_auth_burst_size")]
    pub auth_burst_size: u32,
    /// How many seconds it takes to replenish one request slot for auth routes
    #[serde(default = "RateLimitConfig::default_auth_per_seconds")]
    pub auth_per_seconds: u64,
    /// Max burst of requests for API routes before limiting kicks in
    #[serde(default = "RateLimitConfig::default_api_burst_size")]
    pub api_burst_size: u32,
    /// How many seconds it takes to replenish one request slot for API routes
    #[serde(default = "RateLimitConfig::default_api_per_seconds")]
    pub api_per_seconds: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            auth_burst_size: Self::default_auth_burst_size(),
            auth_per_seconds: Self::default_auth_per_seconds(),
            api_burst_size: Self::default_api_burst_size(),
            api_per_seconds: Self::default_api_per_seconds(),
        }
    }
}

impl RateLimitConfig {
    fn default_auth_burst_size() -> u32 {
        10
    }
    fn default_auth_per_seconds() -> u64 {
        60
    }
    fn default_api_burst_size() -> u32 {
        30
    }
    fn default_api_per_seconds() -> u64 {
        60
    }
}

/// A validated log filter directive string.
/// Uses the same syntax as RUST_LOG (e.g. "info", "my_crate=debug,tower_http=warn").
/// Validated at config parse time so typos blow up early :3
#[derive(Clone, Debug)]
pub struct LogFilter(EnvFilter);

/// A simple max-level filter for the OTel tracing layer.
/// Accepts level names: "trace", "debug", "info", "warn", "error", "off".
#[derive(Clone, Copy, Debug)]
pub struct TracingLevel(pub tracing::level_filters::LevelFilter);

impl Default for LogFilter {
    fn default() -> Self {
        Self(EnvFilter::new("fckn_gay=debug,info"))
    }
}

impl LogFilter {
    /// Converts this validated filter string into a live [`EnvFilter`].
    pub fn into_env_filter(self) -> EnvFilter {
        self.0
    }
}

impl<'de> Deserialize<'de> for LogFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        EnvFilter::try_new(&s)
            .map_err(serde::de::Error::custom)
            .map(LogFilter)
    }
}

impl Default for TracingLevel {
    fn default() -> Self {
        Self(tracing::level_filters::LevelFilter::INFO)
    }
}

impl<'de> Deserialize<'de> for TracingLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<tracing::level_filters::LevelFilter>()
            .map(TracingLevel)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LoggingConfig {
    /// the log level to filter by
    #[serde(default)]
    pub level: LogFilter,
}

#[derive(Clone)]
pub struct PublicSuffix(Arc<String>);
impl PublicSuffix {
    pub fn new(suffix: String) -> Self {
        let suffix = if suffix.starts_with('.') {
            suffix
        } else {
            format!(".{suffix}")
        };
        Self(Arc::new(suffix))
    }
}

impl PublicSuffix {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PublicSuffix {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let suffix = String::deserialize(deserializer)?;
        Ok(PublicSuffix::new(suffix))
    }
}

impl Display for PublicSuffix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

#[derive(Deserialize)]
pub struct TurnstileConfig {
    pub site_key: String,
    pub secret_key: fckn_gay_secret::Secret,
}

#[derive(Deserialize)]
pub struct Config {
    /// the address of the server
    pub address: String,
    pub public_suffix: PublicSuffix,
    pub dns: <Dns as DnsInterface>::Config,
    pub user_database: <UserDatabase as UserDatabaseIntferface>::Config,
    pub email: <Email as EmailInterface>::Config,
    /// Logging configuration - if not specified, uses sensible defaults
    #[serde(default)]
    pub logging: LoggingConfig,
    /// Rate limiting settings - if not specified, uses sensible defaults
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    /// Optional Cloudflare Turnstile captcha for sign-up protection.
    /// If absent, sign-up works without captcha.
    pub turnstile: Option<TurnstileConfig>,
    /// Tracing config — trust_incoming_spans is always available, OTel-specific
    /// fields (backend, endpoint, headers) only compile with --features otel.
    #[serde(default)]
    pub tracing: TracingConfig,
}

impl Config {
    /// Loads the configuration from a file.
    pub fn load_from_file<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Self, std::io::Error> {
        let config_str = std::fs::read_to_string(path.as_ref())?;
        toml::from_str(&config_str)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Builds a fully-dummy config for local dev / testing.
    /// Binds to 127.0.0.1:3000, uses in-memory providers for everything.
    pub fn dummy() -> Self {
        Config {
            address: "127.0.0.1:3000".to_string(),
            public_suffix: PublicSuffix::new("localhost".to_string()),
            dns: fckn_gay_dns::Config::dummy(),
            user_database: fckn_gay_user_database::Config::dummy(),
            email: fckn_gay_email::Config::dummy(),
            logging: LoggingConfig::default(),
            rate_limit: RateLimitConfig::default(),
            turnstile: None,
            tracing: TracingConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TracingConfig {
    /// Which tracing backend to use: "disabled", "stdout", or "otlp".
    #[serde(default)]
    pub provider: crate::telemetry::tracing_setup::TracingBackend,
    /// When true, extract W3C trace context (traceparent/tracestate) from
    /// incoming requests so the request span uses the caller's trace ID.
    /// Only enable when the server sits behind a trusted proxy — accepting
    /// trace IDs from the public internet lets anyone inject into your traces.
    #[serde(default)]
    pub trust_incoming_spans: bool,
    /// How many hex characters of the trace ID to show in log output.
    /// Applies to random IDs, traceparent extraction, and OTel trace IDs.
    #[serde(default = "TracingConfig::default_trace_id_chars", deserialize_with = "deserialize_trace_id_chars")]
    pub trace_id_chars: usize,
    /// Max level for the OTel tracing layer, independent from the logging level.
    /// e.g. gather traces at "debug" while only printing logs at "info".
    #[serde(default)]
    pub level: TracingLevel,
    /// Provider-specific config for the OTLP backend.
    #[serde(default)]
    pub otlp: OtlpConfig,
}

/// OTLP-specific configuration — only used when `provider = "otlp"`.
#[derive(Debug, Deserialize)]
pub struct OtlpConfig {
    #[serde(default = "OtlpConfig::default_endpoint")]
    pub endpoint: String,
    #[serde(default = "OtlpConfig::default_service_name")]
    pub service_name: String,
    /// Custom headers for authenticating to hosted collectors.
    /// Values support all Secret formats: direct string, { env = "VAR" }, { file = "/path" }
    #[serde(default)]
    pub headers: HashMap<String, fckn_gay_secret::Secret>,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            provider: Default::default(),
            trust_incoming_spans: false,
            trace_id_chars: Self::default_trace_id_chars(),
            level: TracingLevel::default(),
            otlp: OtlpConfig::default(),
        }
    }
}

impl TracingConfig {
    fn default_trace_id_chars() -> usize {
        8
    }
}

fn deserialize_trace_id_chars<'de, D: Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    let n = usize::deserialize(d)?;
    if n == 0 {
        return Err(serde::de::Error::custom("trace_id_chars must be at least 1"));
    }
    Ok(n)
}

impl Default for OtlpConfig {
    fn default() -> Self {
        Self {
            endpoint: Self::default_endpoint(),
            service_name: Self::default_service_name(),
            headers: HashMap::new(),
        }
    }
}

impl OtlpConfig {
    fn default_endpoint() -> String {
        "http://localhost:4317".to_string()
    }
    fn default_service_name() -> String {
        "fckn-gay-server".to_string()
    }
}

#[derive(Clone)]
pub struct Interfaces {
    /// The DNS interface for managing DNS records.
    pub dns: Arc<Dns>,
    /// The user database interface for managing user credentials.
    pub user_database: Arc<UserDatabase>,
    /// The email interface for sending emails.
    pub email: Arc<Email>,
    /// a cache for login sessions, gets cleared on server restart
    pub auth_cache: Arc<AuthenticationCache>,
    /// the public suffix of the server
    pub hostname: PublicSuffix,
    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,
    /// Small in-memory cache for password reset tokens
    /// not persisted to disk since they're short-lived and easily regenerated
    pub password_reset_cache: PasswordResetCache,
    /// The address the server binds to, used for constructing URLs in emails etc.
    pub address: ServerAddress,
    /// Cloudflare Turnstile captcha verifier — None means captcha is disabled
    pub turnstile: Option<Arc<TurnstileVerifier>>,
}

impl FromRef<Interfaces> for Arc<Dns> {
    fn from_ref(state: &Interfaces) -> Self {
        state.dns.clone()
    }
}

impl FromRef<Interfaces> for Arc<UserDatabase> {
    fn from_ref(state: &Interfaces) -> Self {
        state.user_database.clone()
    }
}
impl FromRef<Interfaces> for Arc<Email> {
    fn from_ref(state: &Interfaces) -> Self {
        state.email.clone()
    }
}
impl FromRef<Interfaces> for Arc<AuthenticationCache> {
    fn from_ref(state: &Interfaces) -> Self {
        state.auth_cache.clone()
    }
}
impl FromRef<Interfaces> for PasswordResetCache {
    fn from_ref(state: &Interfaces) -> Self {
        state.password_reset_cache.clone()
    }
}

impl FromRef<Interfaces> for PublicSuffix {
    fn from_ref(state: &Interfaces) -> Self {
        state.hostname.clone()
    }
}
impl FromRef<Interfaces> for ServerAddress {
    fn from_ref(state: &Interfaces) -> Self {
        state.address.clone()
    }
}

impl FromRef<Interfaces> for Option<Arc<TurnstileVerifier>> {
    fn from_ref(state: &Interfaces) -> Self {
        state.turnstile.clone()
    }
}

impl Interfaces {
    /// Creates a new instance of `Interfaces` with the given configuration.
    pub fn new(config: Config) -> Result<Self> {
        let dns = Dns::new(config.dns).context("Failed to create DNS interface")?;
        let dns = Arc::new(dns);
        let user_database = UserDatabase::new(config.user_database)
            .context("Failed to create UserDatabase interface")?;
        let user_database = Arc::new(user_database);
        let email = Email::new(config.email).context("Failed to create Email interface")?;
        let email = Arc::new(email);

        // dummy DB + real DNS = data loss city 💀 DNS records will accumulate forever since the
        // in-memory DB forgets everything on restart but Porkbun/etc keeps the actual records.
        if user_database.is_dummy() && !dns.is_dummy() && !dns.is_hickory() {
            tracing::warn!(
                "⚠️  DANGEROUS CONFIG: dummy user database + real DNS provider! \
                 DNS records created will be permanently lost on restart since the dummy DB \
                 doesn't persist. Please use the dummy or hickory DNS provider for testing."
            );
        }

        tracing::info!(
            "Rate limiting configured: auth={}/{} burst/sec, api={}/{} burst/sec",
            config.rate_limit.auth_burst_size,
            config.rate_limit.auth_per_seconds,
            config.rate_limit.api_burst_size,
            config.rate_limit.api_per_seconds
        );

        let turnstile = config.turnstile.map(|tc| {
            tracing::info!("Turnstile captcha enabled (site_key={})", tc.site_key);
            Arc::new(TurnstileVerifier::new(tc.site_key, tc.secret_key))
        });
        if turnstile.is_none() {
            tracing::info!("Turnstile captcha disabled — sign-up is unprotected");
        }

        Ok(Interfaces {
            dns,
            user_database,
            email,
            auth_cache: Arc::new(AuthenticationCache::new()),
            hostname: config.public_suffix,
            rate_limit: config.rate_limit,
            password_reset_cache: PasswordResetCache(Arc::new(AuthenticationCache::new())),
            address: ServerAddress(Arc::new(config.address)),
            turnstile,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example_config_parses() {
        // Test that the example config file can be parsed without errors
        let config_path = "../example_config.toml";
        match Config::load_from_file(config_path) {
            Ok(config) => {
                // Basic validation that the config loaded correctly
                assert!(!config.address.is_empty());
                assert!(!config.public_suffix.to_string().is_empty());
                println!("✅ Example config parsed successfully!");
            }
            Err(e) => {
                panic!("Failed to parse example config: {}", e);
            }
        }
    }
}
