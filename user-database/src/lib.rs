//! User database interface wrapper crate with feature-gated implementations.
//!
//! Enable specific database providers via Cargo features:
//! - `dummy`: In-memory testing database (default)
//! - `csv`: CSV file-based database
//! - `diesel`: SQLite database via Diesel ORM

#[cfg(feature = "csv")]
pub use fckn_gay_user_database_csv::Database as CsvDatabase;
#[cfg(feature = "diesel")]
pub use fckn_gay_user_database_diesel::Database as DieselDatabase;
#[cfg(feature = "dummy")]
pub use fckn_gay_user_database_dummy::Database as DummyDatabase;
pub use fckn_gay_user_database_interface::{
    DatabaseDnsRecord, DnsRecordId, UserDatabase as Interface, Uuid,
};
use serde::{Deserialize, Deserializer};

/// A sink type that absorbs any TOML value when a feature is disabled.
/// This lets us keep config files unchanged even if you don't compile in a provider.
#[derive(Debug, Clone)]
pub struct Disabled;

impl<'de> Deserialize<'de> for Disabled {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Nom nom nom, we eat the config and do nothing with it 🍽️
        let _ = serde::de::IgnoredAny::deserialize(deserializer)?;
        Ok(Disabled)
    }
}

pub enum Database {
    #[cfg(feature = "dummy")]
    Dummy(DummyDatabase),
    #[cfg(feature = "csv")]
    Csv(CsvDatabase),
    #[cfg(feature = "diesel")]
    Diesel(DieselDatabase),
}

impl<'de> Deserialize<'de> for Database {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let config = Config::deserialize(deserializer)?;
        Database::new(config).map_err(serde::de::Error::custom)
    }
}

/// Available database providers. All variants exist regardless of feature flags -
/// we check at runtime if the selected provider is actually compiled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Providers {
    Dummy,
    Csv,
    Diesel,
}

impl std::fmt::Display for Providers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Providers::Dummy => write!(f, "dummy"),
            Providers::Csv => write!(f, "csv"),
            Providers::Diesel => write!(f, "diesel"),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    #[cfg(feature = "dummy")]
    Dummy(<DummyDatabase as Interface>::Error),
    #[cfg(feature = "csv")]
    Csv(<CsvDatabase as Interface>::Error),
    #[cfg(feature = "diesel")]
    Diesel(<DieselDatabase as Interface>::Error),
    MissingConfig(&'static str),
    CantChoseProvider,
    /// The selected provider was not compiled into this binary
    ProviderNotCompiled(String),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "dummy")]
            Error::Dummy(err) => err.source(),
            #[cfg(feature = "csv")]
            Error::Csv(err) => err.source(),
            #[cfg(feature = "diesel")]
            Error::Diesel(err) => err.source(),
            Error::MissingConfig(_) => None,
            Error::CantChoseProvider => None,
            Error::ProviderNotCompiled(_) => None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "dummy")]
            Error::Dummy(err) => write!(f, "{err}"),
            #[cfg(feature = "csv")]
            Error::Csv(err) => write!(f, "{err}"),
            #[cfg(feature = "diesel")]
            Error::Diesel(err) => write!(f, "{err}"),
            Error::MissingConfig(msg) => {
                write!(f, "Missing configuration for selected provider: {msg}")
            }
            Error::CantChoseProvider => {
                write!(
                    f,
                    "Multiple providers specified, please choose one with `provider` field or set only one in the config"
                )
            }
            Error::ProviderNotCompiled(provider) => {
                write!(
                    f,
                    "Provider '{provider}' was selected but is not compiled into this binary. \
                     Recompile with the '{provider}' feature enabled."
                )
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub provider: Option<Providers>,
    #[cfg(feature = "dummy")]
    pub dummy: Option<<DummyDatabase as Interface>::Config>,
    #[cfg(not(feature = "dummy"))]
    pub dummy: Option<Disabled>,
    #[cfg(feature = "csv")]
    pub csv: Option<<CsvDatabase as Interface>::Config>,
    #[cfg(not(feature = "csv"))]
    pub csv: Option<Disabled>,
    #[cfg(feature = "diesel")]
    pub diesel: Option<<DieselDatabase as Interface>::Config>,
    #[cfg(not(feature = "diesel"))]
    pub diesel: Option<Disabled>,
}

impl Config {
    /// Warns about provider configs that are present but won't be validated
    /// because the feature isn't compiled in.
    fn warn_uncompiled_providers(&self) {
        #[cfg(not(feature = "dummy"))]
        if self.dummy.is_some() {
            eprintln!(
                "⚠️  Warning: [user_database.dummy] config present but 'dummy' feature not compiled in - config won't be validated"
            );
        }
        #[cfg(not(feature = "csv"))]
        if self.csv.is_some() {
            eprintln!(
                "⚠️  Warning: [user_database.csv] config present but 'csv' feature not compiled in - config won't be validated"
            );
        }
        #[cfg(not(feature = "diesel"))]
        if self.diesel.is_some() {
            eprintln!(
                "⚠️  Warning: [user_database.diesel] config present but 'diesel' feature not compiled in - config won't be validated"
            );
        }
    }

    /// Returns which provider is active, checking ALL config sections regardless of feature flags.
    /// This ensures config behavior is consistent no matter which features are compiled in.
    fn active(&self) -> Result<Providers, Error> {
        if let Some(provider) = &self.provider {
            return Ok(*provider);
        }

        // Count ALL present configs, including disabled ones
        let mut found: Option<Providers> = None;
        let mut count = 0;

        if self.dummy.is_some() {
            found = Some(Providers::Dummy);
            count += 1;
        }
        if self.csv.is_some() {
            found = Some(Providers::Csv);
            count += 1;
        }
        if self.diesel.is_some() {
            found = Some(Providers::Diesel);
            count += 1;
        }

        match (found, count) {
            (Some(p), 1) => Ok(p),
            (None, 0) => Err(Error::MissingConfig("No provider specified")),
            _ => Err(Error::CantChoseProvider),
        }
    }
}

impl Interface for Database {
    type Config = Config;
    type Error = Error;

    fn new(config: Config) -> Result<Self, Self::Error> {
        // Warn about configs that won't be validated
        config.warn_uncompiled_providers();

        let selected = config.active()?;

        // Check if the selected provider is compiled in
        match selected {
            #[cfg(not(feature = "dummy"))]
            Providers::Dummy => return Err(Error::ProviderNotCompiled("dummy".to_string())),
            #[cfg(not(feature = "csv"))]
            Providers::Csv => return Err(Error::ProviderNotCompiled("csv".to_string())),
            #[cfg(not(feature = "diesel"))]
            Providers::Diesel => {
                return Err(Error::ProviderNotCompiled("diesel".to_string()));
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }

        match selected {
            #[cfg(feature = "dummy")]
            Providers::Dummy => {
                DummyDatabase::new(config.dummy.ok_or(Error::MissingConfig("Dummy"))?)
                    .map_err(Error::Dummy)
                    .map(Database::Dummy)
            }
            #[cfg(feature = "csv")]
            Providers::Csv => CsvDatabase::new(config.csv.ok_or(Error::MissingConfig("Csv"))?)
                .map_err(Error::Csv)
                .map(Database::Csv),
            #[cfg(feature = "diesel")]
            Providers::Diesel => {
                DieselDatabase::new(config.diesel.ok_or(Error::MissingConfig("Diesel"))?)
                    .map_err(Error::Diesel)
                    .map(Database::Diesel)
            }
            #[allow(unreachable_patterns)]
            _ => unreachable!("Provider availability was already checked above"),
        }
    }

    async fn is_valid(&self, username: &str, password: &str) -> bool {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db.is_valid(username, password).await,
            #[cfg(feature = "csv")]
            Database::Csv(db) => db.is_valid(username, password).await,
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db.is_valid(username, password).await,
        }
    }

    async fn validate_and_get_user_id(&self, username: &str, password: &str) -> Option<Uuid> {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db.validate_and_get_user_id(username, password).await,
            #[cfg(feature = "csv")]
            Database::Csv(db) => db.validate_and_get_user_id(username, password).await,
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db.validate_and_get_user_id(username, password).await,
        }
    }

    async fn is_available(&self, username: &str) -> bool {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db.is_available(username).await,
            #[cfg(feature = "csv")]
            Database::Csv(db) => db.is_available(username).await,
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db.is_available(username).await,
        }
    }

    async fn add_user(
        &self,
        username: &str,
        password: &str,
        email: &str,
    ) -> Result<fckn_gay_user_database_interface::Uuid, Self::Error> {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db
                .add_user(username, password, email)
                .await
                .map_err(Self::Error::Dummy),
            #[cfg(feature = "csv")]
            Database::Csv(db) => db
                .add_user(username, password, email)
                .await
                .map_err(Self::Error::Csv),
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db
                .add_user(username, password, email)
                .await
                .map_err(Self::Error::Diesel),
        }
    }

    async fn activate_user(&self, uuid: Uuid) -> Result<(), Self::Error> {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db.activate_user(uuid).await.map_err(Self::Error::Dummy),
            #[cfg(feature = "csv")]
            Database::Csv(db) => db.activate_user(uuid).await.map_err(Self::Error::Csv),
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db.activate_user(uuid).await.map_err(Self::Error::Diesel),
        }
    }

    async fn add_dns_record(
        &self,
        user_id: Uuid,
        record: fckn_gay_user_database_interface::DnsRecord,
        provider_key: String,
    ) -> Result<DnsRecordId, Self::Error> {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db
                .add_dns_record(user_id, record, provider_key)
                .await
                .map_err(Self::Error::Dummy),
            #[cfg(feature = "csv")]
            Database::Csv(db) => db
                .add_dns_record(user_id, record, provider_key)
                .await
                .map_err(Self::Error::Csv),
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db
                .add_dns_record(user_id, record, provider_key)
                .await
                .map_err(Self::Error::Diesel),
        }
    }

    async fn get_user_dns_records(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<fckn_gay_user_database_interface::DatabaseDnsRecord>, Self::Error> {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db
                .get_user_dns_records(user_id)
                .await
                .map_err(Self::Error::Dummy),
            #[cfg(feature = "csv")]
            Database::Csv(db) => db
                .get_user_dns_records(user_id)
                .await
                .map_err(Self::Error::Csv),
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db
                .get_user_dns_records(user_id)
                .await
                .map_err(Self::Error::Diesel),
        }
    }

    async fn update_dns_record(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
        record: fckn_gay_user_database_interface::DnsRecord,
    ) -> Result<(), Self::Error> {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db
                .update_dns_record(user_id, record_id, record)
                .await
                .map_err(Self::Error::Dummy),
            #[cfg(feature = "csv")]
            Database::Csv(db) => db
                .update_dns_record(user_id, record_id, record)
                .await
                .map_err(Self::Error::Csv),
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db
                .update_dns_record(user_id, record_id, record)
                .await
                .map_err(Self::Error::Diesel),
        }
    }

    async fn delete_dns_record(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
    ) -> Result<(), Self::Error> {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db
                .delete_dns_record(user_id, record_id)
                .await
                .map_err(Self::Error::Dummy),
            #[cfg(feature = "csv")]
            Database::Csv(db) => db
                .delete_dns_record(user_id, record_id)
                .await
                .map_err(Self::Error::Csv),
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db
                .delete_dns_record(user_id, record_id)
                .await
                .map_err(Self::Error::Diesel),
        }
    }

    async fn get_dns_record_provider_key(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
    ) -> Result<String, Self::Error> {
        match self {
            #[cfg(feature = "dummy")]
            Database::Dummy(db) => db
                .get_dns_record_provider_key(user_id, record_id)
                .await
                .map_err(Self::Error::Dummy),
            #[cfg(feature = "csv")]
            Database::Csv(db) => db
                .get_dns_record_provider_key(user_id, record_id)
                .await
                .map_err(Self::Error::Csv),
            #[cfg(feature = "diesel")]
            Database::Diesel(db) => db
                .get_dns_record_provider_key(user_id, record_id)
                .await
                .map_err(Self::Error::Diesel),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_with_deny_unknown_fields() {
        // This should parse fine - only known fields
        let toml_str = r#"
            provider = "dummy"
            [dummy]
            test = "password"
        "#;
        let config: Result<Config, _> = toml::from_str(toml_str);
        assert!(config.is_ok(), "Valid config should parse: {:?}", config);
    }

    #[test]
    fn test_config_rejects_unknown_fields() {
        // This should fail - unknown field at root level
        let toml_str = r#"
            provider = "dummy"
            unknown_field = "oops"
            [dummy]
            test = "password"
        "#;
        let config: Result<Config, _> = toml::from_str(toml_str);
        assert!(config.is_err(), "Unknown fields should be rejected");
    }

    #[test]
    fn test_ambiguous_config_errors() {
        // Multiple providers without explicit selection should error
        let toml_str = r#"
            [dummy]
            test = "password"
            [diesel]
            database_url = "sqlite.db"
        "#;
        let config: Config = toml::from_str(toml_str).expect("Should parse");
        let result = config.active();
        assert!(
            matches!(result, Err(Error::CantChoseProvider)),
            "Should error on ambiguous config: {:?}",
            result
        );
    }

    #[test]
    fn test_explicit_provider_resolves_ambiguity() {
        // Explicit provider should work even with multiple configs
        let toml_str = r#"
            provider = "dummy"
            [dummy]
            test = "password"
            [diesel]
            database_url = "sqlite.db"
        "#;
        let config: Config = toml::from_str(toml_str).expect("Should parse");
        let result = config.active();
        assert!(
            matches!(result, Ok(Providers::Dummy)),
            "Explicit provider should work: {:?}",
            result
        );
    }
}
