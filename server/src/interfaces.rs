use std::{fmt::Display, path::Path, sync::Arc};

use anyhow::{Context, Result};
use axum::extract::FromRef;
use fckn_gay_dns::{Dns, Interface as DnsInterface};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseIntferface};
use serde::{Deserialize, Deserializer};

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
    /// Rate limiting settings - if not specified, uses sensible defaults
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    /// Optional Cloudflare Turnstile captcha for sign-up protection.
    /// If absent, sign-up works without captcha.
    pub turnstile: Option<TurnstileConfig>,
}

impl Config {
    /// Loads the configuration from a file.
    pub fn load_from_file<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Self, std::io::Error> {
        let config_str = std::fs::read_to_string(path.as_ref())?;
        toml::from_str(&config_str)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
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

        log::info!(
            "Rate limiting configured: auth={}/{} burst/sec, api={}/{} burst/sec",
            config.rate_limit.auth_burst_size,
            config.rate_limit.auth_per_seconds,
            config.rate_limit.api_burst_size,
            config.rate_limit.api_per_seconds
        );

        let turnstile = config.turnstile.map(|tc| {
            log::info!("Turnstile captcha enabled (site_key={})", tc.site_key);
            Arc::new(TurnstileVerifier::new(tc.site_key, tc.secret_key))
        });
        if turnstile.is_none() {
            log::info!("Turnstile captcha disabled — sign-up is unprotected");
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
