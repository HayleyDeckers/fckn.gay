use std::{fmt::Display, sync::Arc};

use anyhow::{Context, Result};
use axum::extract::FromRef;
use fckn_gay_dns::{Dns, Interface as DnsInterface, Record};
use fckn_gay_email::{Email, Interface as EmailInterface};
// Use types through the main crates
use fckn_gay_user_database::DatabaseDnsRecord;
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseIntferface};
use serde::{Deserialize, Deserializer};

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
    /// DNS migration configuration
    #[serde(default)]
    pub dns_migration: DnsMigrationConfig,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct DnsMigrationConfig {
    /// Whether to enable DNS record sync at startup
    #[serde(default)]
    pub enabled: bool,
    /// What to do when a record exists in provider but not in database
    #[serde(default)]
    pub provider_vs_database: ProviderVsDatabaseStrategy,
    /// What to do when a record exists in database but assigned to different provider
    #[serde(default)]
    pub different_provider: DifferentProviderStrategy,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub enum ProviderVsDatabaseStrategy {
    /// Database wins - delete from provider
    #[default]
    DatabaseWins,
    /// Provider wins - add to database
    ProviderWins,
    /// Skip and log warning
    Skip,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub enum DifferentProviderStrategy {
    /// Move the record to current provider (remove from old, add to new, update database)
    #[default]
    Move,
    /// Duplicate the record (add new record with different key)
    Duplicate,
    /// Skip and do nothing
    Skip,
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
    pub dns: Arc<Dns>,
    /// The user database interface for managing user credentials.
    pub user_database: Arc<UserDatabase>,
    /// The email interface for sending emails.
    pub email: Arc<Email>,
    /// a cache for login sessions, gets cleared on server restart
    pub auth_cache: Arc<crate::auth_cache::AuthenticationCache>,
    pub hostname: PublicSuffix,
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
        let dns = Arc::new(dns);
        let user_database = UserDatabase::new(config.user_database)
            .context("Failed to create UserDatabase interface")?;
        let user_database = Arc::new(user_database);
        let email = Email::new(config.email).context("Failed to create Email interface")?;
        let email = Arc::new(email);

        let interfaces = Interfaces {
            dns,
            user_database,
            email,
            auth_cache: Arc::new(crate::auth_cache::AuthenticationCache::new()),
            hostname: config.public_suffix,
        };

        // Run DNS sync if enabled
        if config.dns_migration.enabled {
            println!("🔄 Starting DNS record sync...");
            let sync_result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(interfaces.sync_dns_records(&config.dns_migration))
            });
            if let Err(e) = sync_result {
                eprintln!("⚠️  DNS sync failed: {}", e);
                // Don't fail startup, just log the error
            } else {
                println!("✅ DNS sync completed successfully!");
            }
        }

        Ok(interfaces)
    }

    /// Syncs DNS records between the current provider and database
    async fn sync_dns_records(&self, config: &DnsMigrationConfig) -> Result<()> {
        // Get current provider name
        let current_provider = self.get_current_provider_name()?;
        println!("🎯 Current provider: {}", current_provider);

        // Get all records from the current provider
        let provider_records = self.dns.list_records().await?;
        println!("📊 Found {} records in provider", provider_records.len());

        // Get all records from the database
        let db_records: Vec<DatabaseDnsRecord> = self.get_all_dns_records().await?;
        println!("📊 Found {} records in database", db_records.len());

        // Filter database records to only those that should be in the current provider
        let current_provider_records: Vec<_> = db_records
            .into_iter()
            .filter(|record| record.provider_key.starts_with(current_provider))
            .collect();

        println!(
            "📊 Found {} records assigned to current provider in database",
            current_provider_records.len()
        );

        // Handle records that exist in provider but not in database
        self.handle_provider_vs_database_conflicts(
            &provider_records,
            &current_provider_records,
            config,
        )
        .await?;

        // Handle records that exist in database but assigned to different provider
        self.handle_different_provider_records(&current_provider_records, config)
            .await?;

        Ok(())
    }

    /// Gets all DNS records from the database
    async fn get_all_dns_records(&self) -> Result<Vec<DatabaseDnsRecord>> {
        self.user_database
            .get_all_dns_records()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get DNS records: {}", e))
    }

    /// Gets the current provider name
    fn get_current_provider_name(&self) -> Result<&'static str> {
        // Extract provider name from the DNS interface
        match &*self.dns {
            fckn_gay_dns::Dns::Porkbun(_) => Ok("Porkbun"),
            fckn_gay_dns::Dns::Dummy(_) => Ok("Dummy"),
            fckn_gay_dns::Dns::Hickory(_) => Ok("Hickory"),
        }
    }

    /// Handles conflicts between provider and database records
    async fn handle_provider_vs_database_conflicts(
        &self,
        provider_records: &[(fckn_gay_dns::Key, Record)],
        db_records: &[DatabaseDnsRecord],
        config: &DnsMigrationConfig,
    ) -> Result<()> {
        // Find records that exist in provider but not in database
        // todo: handle the case where a record exists in the database but is not assigned to the current provider
        let provider_only_records: Vec<_> = provider_records
            .iter()
            .filter(|(_, record)| !db_records.iter().any(|db| db.record == *record))
            .collect();

        if provider_only_records.is_empty() {
            println!("✅ No provider-only records found");
            return Ok(());
        }

        println!(
            "🔍 Found {} records in provider but not in database",
            provider_only_records.len()
        );

        match config.provider_vs_database {
            ProviderVsDatabaseStrategy::DatabaseWins => {
                println!(
                    "🗑️  Deleting {} records from provider (database wins)",
                    provider_only_records.len()
                );
                for (key, record) in provider_only_records {
                    println!("🗑️  Deleting: {} {}", record.name, record.record_type);
                    if let Err(e) = self.dns.delete_record(key.clone()).await {
                        eprintln!(
                            "❌ Failed to delete record {} {}: {}",
                            record.name, record.record_type, e
                        );
                    }
                }
            }
            ProviderVsDatabaseStrategy::ProviderWins => {
                println!(
                    "➕ Adding {} records to database (provider wins)",
                    provider_only_records.len()
                );
                for (_key, record) in provider_only_records {
                    println!(
                        "➕ Adding to database: {} {}",
                        record.name, record.record_type
                    );
                    // TODO: Add to database - we'd need a user_id for this
                    // we'd need to infer the user_id from the record name and the public suffix
                    // meaning we need some way to query the database for the user_id by name.
                    eprintln!("⚠️  Adding records to database not implemented yet");
                }
            }
            ProviderVsDatabaseStrategy::Skip => {
                println!(
                    "⏭️  Skipping {} provider-only records",
                    provider_only_records.len()
                );
            }
        }
        //todo: handle the case where the database has records that the provider doesn't have

        Ok(())
    }

    /// Handles records that exist in database but assigned to different provider
    async fn handle_different_provider_records(
        &self,
        current_provider_records: &[DatabaseDnsRecord],
        config: &DnsMigrationConfig,
    ) -> Result<()> {
        let current_provider = self.get_current_provider_name()?;
        // Find records that should be in current provider but aren't
        let missing_records: Vec<_> = current_provider_records
            .iter()
            .filter(|record| record.provider_key != current_provider)
            .collect();

        if missing_records.is_empty() {
            println!("✅ All database records are present in provider");
            return Ok(());
        }

        println!(
            "🔍 Found {} records in database but missing from provider",
            missing_records.len()
        );

        match config.different_provider {
            DifferentProviderStrategy::Move | DifferentProviderStrategy::Duplicate => {
                println!(
                    "🔄 Copying {} records to current provider",
                    missing_records.len()
                );
                for record in missing_records {
                    println!(
                        "➕ Adding to provider: {} {}",
                        record.record.name, record.record.record_type
                    );
                    // add the record to the current provider
                    match self.dns.add_record(record.record.clone()).await {
                        Err(e) => {
                            eprintln!(
                                "❌ Failed to add record {} {}: {}",
                                record.record.name, record.record.record_type, e
                            );
                        }
                        Ok(key) => {
                            // if we succeeded, change the provider key in the database
                            if let Err(e) = self
                                .user_database
                                .update_dns_record_provider_key(record.id.clone(), key.to_string())
                                .await
                            {
                                eprintln!(
                                    "❌ Failed to update database record provider key: {}",
                                    e
                                );
                                // rollback the record addition in the DNS provider
                                if let Err(e) = self.dns.delete_record(key.clone()).await {
                                    eprintln!(
                                        "❌ Failed to rollback record addition in DNS provider: {}",
                                        e
                                    );
                                }
                            }
                            println!(
                                "✅ Added record: {} {}=> {}",
                                record.record.name, record.record.record_type, key
                            );
                            if let DifferentProviderStrategy::Move = config.different_provider {
                                // todo: delete the record from the old provider
                                eprintln!(
                                    "⚠️  Deleting record from old provider not implemented yet"
                                );
                            }
                        }
                    }
                }
            }
            DifferentProviderStrategy::Skip => {
                println!("⏭️  Skipping {} missing records", missing_records.len());
            }
        }

        Ok(())
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
