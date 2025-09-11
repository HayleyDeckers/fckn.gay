pub use fckn_gay_user_database_csv::Database as CsvDatabase;
pub use fckn_gay_user_database_diesel::Database as DieselDatabase;
pub use fckn_gay_user_database_hardcoded::Database as HardcodedDatabase;
pub use fckn_gay_user_database_interface::{UserDatabase as Interface, Uuid};
use serde::Deserialize;

pub enum Database {
    Hardcoded(HardcodedDatabase),
    Csv(CsvDatabase),
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Providers {
    Hardcoded,
    Csv,
    Diesel,
}

#[derive(Debug)]
pub enum Error {
    Hardcoded(<HardcodedDatabase as Interface>::Error),
    Csv(<CsvDatabase as Interface>::Error),
    Diesel(<DieselDatabase as Interface>::Error),
    MissingConfig(&'static str),
    CantChoseProvider,
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Hardcoded(err) => err.source(),
            Error::Csv(err) => err.source(),
            Error::Diesel(err) => err.source(),
            Error::MissingConfig(_) => None,
            Error::CantChoseProvider => None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Hardcoded(err) => write!(f, "{}", err),
            Error::Csv(err) => write!(f, "{}", err),
            Error::Diesel(err) => write!(f, "{}", err),
            Error::MissingConfig(msg) => {
                write!(f, "Missing configuration for selected provider: {}", msg)
            }
            Error::CantChoseProvider => {
                write!(
                    f,
                    "Multiple providers specified, please choose one with `provider` field or set only one in the config"
                )
            }
        }
    }
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub provider: Option<Providers>,
    pub hardcoded: Option<<HardcodedDatabase as Interface>::Config>,
    pub csv: Option<<CsvDatabase as Interface>::Config>,
    pub diesel: Option<<DieselDatabase as Interface>::Config>,
}

impl Interface for Database {
    type Config = Config;
    type Error = Error;
    fn new(config: Config) -> Result<Self, Self::Error> {
        let Config {
            provider,
            hardcoded,
            csv,
            diesel,
        } = config;
        if let Some(provider) = provider {
            match provider {
                Providers::Hardcoded => {
                    HardcodedDatabase::new(hardcoded.ok_or(Error::MissingConfig("Hardcoded"))?)
                        .map_err(Error::Hardcoded)
                        .map(Database::Hardcoded)
                }
                Providers::Csv => CsvDatabase::new(csv.ok_or(Error::MissingConfig("Csv"))?)
                    .map_err(Error::Csv)
                    .map(Database::Csv),
                Providers::Diesel => {
                    DieselDatabase::new(diesel.ok_or(Error::MissingConfig("Diesel"))?)
                        .map_err(Error::Diesel)
                        .map(Database::Diesel)
                }
            }
        } else {
            match (hardcoded, csv, diesel) {
                (Some(hardcoded), None, None) => HardcodedDatabase::new(hardcoded)
                    .map_err(Error::Hardcoded)
                    .map(Database::Hardcoded),
                (None, Some(csv), None) => {
                    CsvDatabase::new(csv).map_err(Error::Csv).map(Database::Csv)
                }
                (None, None, Some(diesel)) => DieselDatabase::new(diesel)
                    .map_err(Error::Diesel)
                    .map(Database::Diesel),
                (None, None, None) => Err(Error::MissingConfig("No provider specified")),
                _ => Err(Error::CantChoseProvider),
            }
        }
    }
    async fn is_valid(&self, username: &str, password: &str) -> bool {
        match self {
            Database::Hardcoded(db) => db.is_valid(username, password).await,
            Database::Csv(db) => db.is_valid(username, password).await,
            Database::Diesel(db) => db.is_valid(username, password).await,
        }
    }

    async fn is_available(&self, username: &str) -> bool {
        match self {
            Database::Hardcoded(db) => db.is_available(username).await,
            Database::Csv(db) => db.is_available(username).await,
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
            Database::Hardcoded(db) => db
                .add_user(username, password, email)
                .await
                .map_err(Self::Error::Hardcoded),
            Database::Csv(db) => db
                .add_user(username, password, email)
                .await
                .map_err(Self::Error::Csv),
            Database::Diesel(db) => db
                .add_user(username, password, email)
                .await
                .map_err(Self::Error::Diesel),
        }
    }

    async fn activate_user(&self, uuid: Uuid) -> Result<(), Self::Error> {
        match self {
            Database::Hardcoded(db) => db.activate_user(uuid).await.map_err(Self::Error::Hardcoded),
            Database::Csv(db) => db.activate_user(uuid).await.map_err(Self::Error::Csv),
            Database::Diesel(db) => db.activate_user(uuid).await.map_err(Self::Error::Diesel),
        }
    }
}
