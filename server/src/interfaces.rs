use std::{fmt::Display, sync::Arc};

use anyhow::{Context, Result};
use axum::extract::FromRef;
use fckn_gay_dns::{Dns, Interface as DnsInterface};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseIntferface};
use serde::{Deserialize, Deserializer};
use tokio::sync::Mutex;

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
pub struct Config {
    /// the address of the server
    pub address: String,
    pub public_suffix: PublicSuffix,
    pub dns: <Dns as DnsInterface>::Config,
    pub user_database: <UserDatabase as UserDatabaseIntferface>::Config,
    pub email: <Email as EmailInterface>::Config,
}

impl Config {
    /// Loads the configuration from a file.
    pub fn load_from_file(path: &str) -> Result<Self, std::io::Error> {
        let config_str = std::fs::read_to_string(path)?;
        toml::from_str(&config_str)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

#[derive(Clone)]
pub struct Interfaces {
    /// The DNS interface for managing DNS records.
    pub dns: Arc<Mutex<Dns>>,
    /// The user database interface for managing user credentials.
    pub user_database: Arc<Mutex<UserDatabase>>,
    /// The email interface for sending emails.
    pub email: Arc<Mutex<Email>>,
    /// a cache for login sessions, gets cleared on server restart
    pub auth_cache: Arc<crate::auth_cache::AuthenticationCache>,
    pub hostname: PublicSuffix,
}

impl FromRef<Interfaces> for Arc<Mutex<Dns>> {
    fn from_ref(state: &Interfaces) -> Self {
        state.dns.clone()
    }
}

impl FromRef<Interfaces> for Arc<Mutex<UserDatabase>> {
    fn from_ref(state: &Interfaces) -> Self {
        state.user_database.clone()
    }
}
impl FromRef<Interfaces> for Arc<Mutex<Email>> {
    fn from_ref(state: &Interfaces) -> Self {
        state.email.clone()
    }
}
impl FromRef<Interfaces> for Arc<crate::auth_cache::AuthenticationCache> {
    fn from_ref(state: &Interfaces) -> Self {
        state.auth_cache.clone()
    }
}

impl FromRef<Interfaces> for PublicSuffix {
    fn from_ref(state: &Interfaces) -> Self {
        state.hostname.clone()
    }
}

impl Interfaces {
    /// Creates a new instance of `Interfaces` with the given configuration.
    pub fn new(config: Config) -> Result<Self> {
        let dns = Dns::new(config.dns).context("Failed to create DNS interface")?;
        let dns = Arc::new(Mutex::new(dns));
        let user_database = UserDatabase::new(config.user_database)
            .context("Failed to create UserDatabase interface")?;
        let user_database = Arc::new(Mutex::new(user_database));
        let email = Email::new(config.email).context("Failed to create Email interface")?;
        let email = Arc::new(Mutex::new(email));

        Ok(Interfaces {
            dns,
            user_database,
            email,
            auth_cache: Arc::new(crate::auth_cache::AuthenticationCache::new()),
            hostname: config.public_suffix,
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
