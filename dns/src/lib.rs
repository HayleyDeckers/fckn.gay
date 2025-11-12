use core::panic;
use std::fmt::Display;

pub use fckn_gay_dns_dummy::DummyDns as Dummy;
pub use fckn_gay_dns_hickory::HickoryDns as Hickory;
pub use fckn_gay_dns_interface::{Dns as Interface, Record, RecordType};
pub use fckn_gay_dns_porkbun::PorkbunDns as Porkbun;
pub use serde::{Deserialize, Serialize};
use serde::{Deserializer, Serializer};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Providers {
    Porkbun,
    Dummy,
    Hickory,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    provider: Option<Providers>,
    porkbun: Option<<Porkbun as Interface>::Config>,
    dummy: Option<<Dummy as Interface>::Config>,
    hickory: Option<<Hickory as Interface>::Config>,
}

impl Config {
    pub fn active(&self) -> Result<Providers, Error> {
        if let Some(provider) = self.provider {
            Ok(provider)
        } else {
            match (&self.porkbun, &self.dummy, &self.hickory) {
                (Some(_), None, None) => Ok(Providers::Porkbun),
                (None, Some(_), None) => Ok(Providers::Dummy),
                (None, None, Some(_)) => Ok(Providers::Hickory),
                (None, None, None) => Err(Error::NoConfig),
                _ => Err(Error::CantChoseProvider),
            }
        }
    }
}

pub enum ActiveDns {
    Porkbun(Porkbun),
    Dummy(Dummy),
    Hickory(Hickory),
}

impl ActiveDns {
    pub fn porkbun(&self) -> Option<&Porkbun> {
        match self {
            ActiveDns::Porkbun(porkbun) => Some(porkbun),
            _ => None,
        }
    }
    pub fn dummy(&self) -> Option<&Dummy> {
        match self {
            ActiveDns::Dummy(dummy) => Some(dummy),
            _ => None,
        }
    }
    pub fn hickory(&self) -> Option<&Hickory> {
        match self {
            ActiveDns::Hickory(hickory) => Some(hickory),
            _ => None,
        }
    }
}

pub struct Dns {
    active: ActiveDns,
    porkbun: Option<Porkbun>,
    dummy: Option<Dummy>,
    hickory: Option<Hickory>,
}

impl Dns {
    pub fn porkbun(&self) -> Result<&Porkbun, Error> {
        self.active
            .porkbun()
            .or(self.porkbun.as_ref())
            .ok_or(Error::MissingConfig("Porkbun"))
    }
    pub fn dummy(&self) -> Result<&Dummy, Error> {
        self.active
            .dummy()
            .or(self.dummy.as_ref())
            .ok_or(Error::MissingConfig("Dummy"))
    }
    pub fn hickory(&self) -> Result<&Hickory, Error> {
        self.active
            .hickory()
            .or(self.hickory.as_ref())
            .ok_or(Error::MissingConfig("Hickory"))
    }

    /// Returns the currently active provider type.
    pub fn active_provider(&self) -> Providers {
        match &self.active {
            ActiveDns::Porkbun(_) => Providers::Porkbun,
            ActiveDns::Dummy(_) => Providers::Dummy,
            ActiveDns::Hickory(_) => Providers::Hickory,
        }
    }

    /// Generic internal migrate function that works with any DNS provider as source.
    ///
    /// # Arguments
    ///
    /// * `source` - The source DNS provider to migrate from
    /// * `key` - The key of the record to migrate
    ///
    /// # Returns
    ///
    /// A `MigrateState` indicating the result of the migration operation.
    async fn migrate_internal<S>(&self, source: &S, key: <S as Interface>::Key) -> MigrateState
    where
        S: Interface,
        <S as Interface>::Key: PartialEq,
    {
        // Get the record from source provider
        let records = match source.list_records().await {
            Ok(records) => records,
            Err(e) => {
                return MigrateState::NothingChanged(MigrateError::GetRecordFailed(Box::new(e)));
            }
        };

        self.migrate_internal_from_list(source, key, &records).await
    }

    async fn migrate_internal_from_list<S>(
        &self,
        source: &S,
        key: <S as Interface>::Key,
        records: &[(<S as Interface>::Key, Record)],
    ) -> MigrateState
    where
        S: Interface,
        <S as Interface>::Key: PartialEq,
    {
        let record = match records
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, record)| record)
        {
            Some(record) => record,
            None => {
                return MigrateState::NothingChanged(MigrateError::GetRecordFailed(Box::new(
                    Error::RecordNotFound,
                )));
            }
        };

        // Add record to active provider
        let new_key = match &self.active {
            ActiveDns::Porkbun(active) => match active.add_record(record.clone()).await {
                Ok(k) => Key::Porkbun(k),
                Err(e) => {
                    return MigrateState::NothingChanged(MigrateError::AddToActiveFailed(
                        Error::Porkbun(e),
                    ));
                }
            },
            ActiveDns::Dummy(active) => match active.add_record(record.clone()).await {
                Ok(k) => Key::Dummy(k),
                Err(e) => {
                    return MigrateState::NothingChanged(MigrateError::AddToActiveFailed(
                        Error::Dummy(e),
                    ));
                }
            },
            ActiveDns::Hickory(active) => match active.add_record(record.clone()).await {
                Ok(k) => Key::Hickory(k),
                Err(e) => {
                    return MigrateState::NothingChanged(MigrateError::AddToActiveFailed(
                        Error::Hickory(e),
                    ));
                }
            },
        };

        // Delete record from source provider
        match source.delete_record(key).await {
            Ok(()) => MigrateState::Success(new_key),
            Err(e) => MigrateState::AddedButNotDeleted(
                new_key,
                MigrateError::DeleteFromSourceFailed(Box::new(e)),
            ),
        }
    }

    /// Migrates a DNS record from one provider to the active provider.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the record to migrate (must be from a different provider than the active one)
    ///
    /// # Returns
    ///
    /// A `MigrateState` indicating the result of the migration operation.
    pub async fn migrate(&self, key: Key) -> MigrateState {
        // Get active provider type
        let active_provider = self.active_provider();

        if matches!(
            (active_provider, &key),
            (Providers::Porkbun, Key::Porkbun(_))
                | (Providers::Dummy, Key::Dummy(_))
                | (Providers::Hickory, Key::Hickory(_))
        ) {
            return MigrateState::NothingChanged(MigrateError::SameProvider);
        }

        // Call the generic migrate function with the appropriate provider types
        match key {
            Key::Porkbun(inner_key) => {
                let source = match self.porkbun() {
                    Ok(s) => s,
                    Err(e) => {
                        return MigrateState::NothingChanged(MigrateError::GetRecordFailed(
                            Box::new(e),
                        ));
                    }
                };
                self.migrate_internal(source, inner_key).await
            }
            Key::Dummy(inner_key) => {
                let source = match self.dummy() {
                    Ok(s) => s,
                    Err(e) => {
                        return MigrateState::NothingChanged(MigrateError::GetRecordFailed(
                            Box::new(e),
                        ));
                    }
                };
                self.migrate_internal(source, inner_key).await
            }
            Key::Hickory(inner_key) => {
                let source = match self.hickory() {
                    Ok(s) => s,
                    Err(e) => {
                        return MigrateState::NothingChanged(MigrateError::GetRecordFailed(
                            Box::new(e),
                        ));
                    }
                };
                self.migrate_internal(source, inner_key).await
            }
        }
    }
}

/// State-based result of a migration operation.
pub enum MigrateState {
    /// Nothing changed - migration failed before any modifications.
    NothingChanged(MigrateError),
    /// Record was added to the active provider but deletion from source failed.
    AddedButNotDeleted(Key, MigrateError),
    /// Migration completed successfully.
    Success(Key),
}

/// Error that tracks at what stage a migration failed.
#[derive(Debug)]
pub enum MigrateError {
    /// Cannot migrate from the same provider.
    SameProvider,
    /// Failed to get the record from the source provider, nothing changed.
    GetRecordFailed(Box<dyn std::error::Error + Send + Sync>),
    /// Failed to add record to active provider, nothing changed.
    AddToActiveFailed(Error),
    /// Failed to delete record from source provider. Record exists in both source and active.
    DeleteFromSourceFailed(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrateError::SameProvider => {
                write!(f, "Cannot migrate from the same provider.")
            }
            MigrateError::GetRecordFailed(_) => {
                write!(
                    f,
                    "Failed to get record from source provider. Nothing changed."
                )
            }
            MigrateError::AddToActiveFailed(_) => {
                write!(
                    f,
                    "Failed to add record to active provider. Nothing changed."
                )
            }
            MigrateError::DeleteFromSourceFailed(_) => {
                write!(
                    f,
                    "Failed to delete record from source provider. Record still exists in both source and active."
                )
            }
        }
    }
}

impl std::error::Error for MigrateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MigrateError::SameProvider => None,
            MigrateError::GetRecordFailed(err) => err.source(),
            MigrateError::AddToActiveFailed(err) => err.source(),
            MigrateError::DeleteFromSourceFailed(err) => err.source(),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Porkbun(<Porkbun as Interface>::Error),
    Dummy(<Dummy as Interface>::Error),
    Hickory(<Hickory as Interface>::Error),
    MissingConfig(&'static str),
    CantChoseProvider,
    NoConfig,
    SameProvider,
    RecordNotFound,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Porkbun(err) => write!(f, "{err}"),
            Error::Dummy(err) => write!(f, "{err}"),
            Error::Hickory(err) => write!(f, "{err}"),
            Error::MissingConfig(msg) => {
                write!(f, "Missing configuration for selected provider: {msg}")
            }
            Error::CantChoseProvider => {
                write!(
                    f,
                    "Multiple providers specified, please choose one with `provider` field or set only one in the config"
                )
            }
            Error::NoConfig => write!(f, "No configuration provided"),
            Error::RecordNotFound => write!(f, "Record not found in source provider"),
            Error::SameProvider => write!(f, "Cannot migrate from the same provider"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Porkbun(err) => err.source(),
            Error::Dummy(err) => err.source(),
            Error::Hickory(err) => err.source(),
            Error::MissingConfig(_) => None,
            Error::CantChoseProvider => None,
            Error::NoConfig => None,
            Error::RecordNotFound => None,
            Error::SameProvider => None,
        }
    }
}

pub enum Key {
    Porkbun(<Porkbun as Interface>::Key),
    Dummy(<Dummy as Interface>::Key),
    Hickory(<Hickory as Interface>::Key),
}

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Key::Porkbun(key) => write!(f, "Porkbun:{key}"),
            Key::Dummy(key) => write!(f, "Dummy:{key}"),
            Key::Hickory(key) => write!(f, "Hickory:{key}"),
        }
    }
}

impl Serialize for Key {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_str())
    }
}

impl std::str::FromStr for Key {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((provider, key)) = s.split_once(':') else {
            return Err(String::from("Invalid key format"));
        };
        match provider {
            "Porkbun" => Ok(Key::Porkbun(
                key.parse().map_err(|e| format!("Invalid key {key}: {e}"))?,
            )),
            "Dummy" => Ok(Key::Dummy(
                key.parse().map_err(|e| format!("Invalid key {key}: {e}"))?,
            )),
            "Hickory" => Ok(Key::Hickory(
                key.parse().map_err(|e| format!("Invalid key {key}: {e}"))?,
            )),
            _ => Err(String::from("Invalid provider")),
        }
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Interface for Dns {
    type Config = Config;
    type Error = Error;
    type Key = Key;

    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        let active = config.active()?;

        let mut porkbun = config.porkbun.map(Porkbun::new);
        let mut dummy = config.dummy.map(Dummy::new);
        let mut hickory = config.hickory.map(Hickory::new);

        // set the active dns
        let active = match active {
            Providers::Porkbun => {
                let porkbun = porkbun
                    .take()
                    .ok_or(Error::MissingConfig("Porkbun"))?
                    .map_err(Error::Porkbun)?;
                ActiveDns::Porkbun(porkbun)
            }
            Providers::Dummy => {
                let dummy = dummy
                    .take()
                    .ok_or(Error::MissingConfig("Dummy"))?
                    .map_err(Error::Dummy)?;
                ActiveDns::Dummy(dummy)
            }
            Providers::Hickory => {
                let hickory = hickory
                    .take()
                    .ok_or(Error::MissingConfig("Hickory"))?
                    .map_err(Error::Hickory)?;
                ActiveDns::Hickory(hickory)
            }
        };

        let porkbun = match porkbun {
            Some(Ok(porkbun)) => Some(porkbun),
            Some(Err(e)) => {
                eprintln!("Error creating DNS provider (Porkbun): {e}");
                None
            }
            None => None,
        };
        let dummy = match dummy {
            Some(Ok(dummy)) => Some(dummy),
            Some(Err(e)) => {
                eprintln!("Error creating DNS provider (Dummy): {e}");
                None
            }
            None => None,
        };
        let hickory = match hickory {
            Some(Ok(hickory)) => Some(hickory),
            Some(Err(e)) => {
                eprintln!("Error creating DNS provider (Hickory): {e}");
                None
            }
            None => None,
        };
        Ok(Dns {
            active,
            porkbun,
            dummy,
            hickory,
        })
    }

    async fn add_record(
        &self,
        record: fckn_gay_dns_interface::Record,
    ) -> Result<Self::Key, Self::Error> {
        match &self.active {
            ActiveDns::Porkbun(porkbun) => porkbun
                .add_record(record)
                .await
                .map(Key::Porkbun)
                .map_err(Error::Porkbun),
            ActiveDns::Dummy(dummy) => dummy
                .add_record(record)
                .await
                .map(Key::Dummy)
                .map_err(Error::Dummy),
            ActiveDns::Hickory(hickory) => hickory
                .add_record(record)
                .await
                .map(Key::Hickory)
                .map_err(Error::Hickory),
        }
    }

    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error> {
        match (&self.active, key) {
            (ActiveDns::Porkbun(porkbun), Key::Porkbun(porkbun_key)) => porkbun
                .delete_record(porkbun_key)
                .await
                .map_err(Error::Porkbun),
            (ActiveDns::Hickory(hickory), Key::Hickory(hickory_key)) => hickory
                .delete_record(hickory_key)
                .await
                .map_err(Error::Hickory),
            (ActiveDns::Dummy(dummy), Key::Dummy(dummy_key)) => {
                dummy.delete_record(dummy_key).await.map_err(Error::Dummy)
            }
            _ => panic!("Invalid key type for DNS provider"),
        }
    }

    async fn list_records(
        &self,
    ) -> Result<Vec<(Self::Key, fckn_gay_dns_interface::Record)>, Self::Error> {
        match &self.active {
            ActiveDns::Porkbun(porkbun) => porkbun
                .list_records()
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(|(key, record)| (Key::Porkbun(key), record))
                        .collect()
                })
                .map_err(Error::Porkbun),
            ActiveDns::Dummy(dummy) => dummy
                .list_records()
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(|(key, record)| (Key::Dummy(key), record))
                        .collect()
                })
                .map_err(Error::Dummy),
            ActiveDns::Hickory(hickory) => hickory
                .list_records()
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(|(key, record)| (Key::Hickory(key), record))
                        .collect()
                })
                .map_err(Error::Hickory),
        }
    }

    async fn update_record(
        &self,
        key: Self::Key,
        record: fckn_gay_dns_interface::Record,
    ) -> Result<(), Self::Error> {
        match (&self.active, key) {
            (ActiveDns::Porkbun(porkbun), Key::Porkbun(porkbun_key)) => porkbun
                .update_record(porkbun_key, record)
                .await
                .map_err(Error::Porkbun),
            (ActiveDns::Hickory(hickory), Key::Hickory(hickory_key)) => hickory
                .update_record(hickory_key, record)
                .await
                .map_err(Error::Hickory),
            (ActiveDns::Dummy(dummy), Key::Dummy(dummy_key)) => dummy
                .update_record(dummy_key, record)
                .await
                .map_err(Error::Dummy),
            _ => panic!("Invalid key type for DNS provider"),
        }
    }
}
