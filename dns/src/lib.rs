use core::panic;

pub use fckn_gay_dns_dummy::DummyDns as Dummy;
pub use fckn_gay_dns_interface::{Dns as Interface, Record, RecordType};
pub use fckn_gay_dns_porkbun::PorkbunDns as Porkbun;
pub use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Providers {
    Porkbun,
    Dummy,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    provider: Option<Providers>,
    porkbun: Option<<Porkbun as Interface>::Config>,
    dummy: Option<<Dummy as Interface>::Config>,
}

pub enum Dns {
    Porkbun(Porkbun),
    Dummy(Dummy),
}

#[derive(Debug)]
pub enum Error {
    Porkbun(<Porkbun as Interface>::Error),
    Dummy(<Dummy as Interface>::Error),
    MissingConfig(&'static str),
    CantChoseProvider,
    NoConfig,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Porkbun(err) => write!(f, "{}", err),
            Error::Dummy(err) => write!(f, "{}", err),
            Error::MissingConfig(msg) => {
                write!(f, "Missing configuration for selected provider: {}", msg)
            }
            Error::CantChoseProvider => {
                write!(
                    f,
                    "Multiple providers specified, please choose one with `provider` field or set only one in the config"
                )
            }
            Error::NoConfig => write!(f, "No configuration provided"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Porkbun(err) => err.source(),
            Error::Dummy(err) => err.source(),
            Error::MissingConfig(_) => None,
            Error::CantChoseProvider => None,
            Error::NoConfig => None,
        }
    }
}

pub enum Key {
    Porkbun(<Porkbun as Interface>::Key),
    Dummy(<Dummy as Interface>::Key),
}

impl Interface for Dns {
    type Config = Config;
    type Error = Error;
    type Key = Key;

    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        if let Some(provider) = config.provider {
            match provider {
                Providers::Porkbun => {
                    Porkbun::new(config.porkbun.ok_or(Error::MissingConfig("Porkbun"))?)
                        .map(Dns::Porkbun)
                        .map_err(Error::Porkbun)
                }
                Providers::Dummy => Dummy::new(config.dummy.ok_or(Error::MissingConfig("Dummy"))?)
                    .map(Dns::Dummy)
                    .map_err(Error::Dummy),
            }
        } else {
            match (config.porkbun, config.dummy) {
                (Some(porkbun), None) => Porkbun::new(porkbun)
                    .map(Dns::Porkbun)
                    .map_err(Error::Porkbun),
                (None, Some(dummy)) => Dummy::new(dummy).map(Dns::Dummy).map_err(Error::Dummy),
                (None, None) => Err(Error::NoConfig),
                _ => Err(Error::CantChoseProvider),
            }
        }
    }

    async fn add_record(
        &self,
        record: fckn_gay_dns_interface::Record,
    ) -> Result<Self::Key, Self::Error> {
        match self {
            Dns::Porkbun(porkbun) => porkbun
                .add_record(record)
                .await
                .map(Key::Porkbun)
                .map_err(Error::Porkbun),
            Dns::Dummy(dummy) => dummy
                .add_record(record)
                .await
                .map(Key::Dummy)
                .map_err(Error::Dummy),
        }
    }

    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error> {
        match (self, key) {
            (Dns::Porkbun(porkbun), Key::Porkbun(porkbun_key)) => porkbun
                .delete_record(porkbun_key)
                .await
                .map_err(Error::Porkbun),
            #[allow(unreachable_patterns)]
            _ => panic!("Invalid key type for DNS provider"),
        }
    }

    async fn list_records(
        &self,
    ) -> Result<Vec<(Self::Key, fckn_gay_dns_interface::Record)>, Self::Error> {
        match self {
            Dns::Porkbun(porkbun) => porkbun
                .list_records()
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(|(key, record)| (Key::Porkbun(key), record))
                        .collect()
                })
                .map_err(Error::Porkbun),
            Dns::Dummy(dummy) => dummy
                .list_records()
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(|(key, record)| (Key::Dummy(key), record))
                        .collect()
                })
                .map_err(Error::Dummy),
        }
    }
}
