use anyhow::{Context, Result};
use axum::extract::FromRef;
use fckn_gay_dns::{Dns, Interface as DnsInterface};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseIntferface};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
pub struct Config {
    /// the address of the server
    pub address: String,
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
        })
    }
}
