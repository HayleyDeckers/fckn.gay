//! DNS interface wrapper crate with feature-gated implementations.
//!
//! Enable specific DNS providers via Cargo features:
//! - `dummy`: In-memory testing provider (default)
//! - `hickory`: Hickory DNS server
//! - `porkbun`: Porkbun API provider

use core::panic;
use std::fmt::Display;

#[cfg(feature = "dummy")]
pub use fckn_gay_dns_dummy::DummyDns as Dummy;
#[cfg(feature = "hickory")]
pub use fckn_gay_dns_hickory::HickoryDns as Hickory;
pub use fckn_gay_dns_interface::{Dns as Interface, Record, RecordType};
#[cfg(feature = "porkbun")]
pub use fckn_gay_dns_porkbun::PorkbunDns as Porkbun;
pub use serde::{Deserialize, Serialize};
use serde::{Deserializer, Serializer};

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

/// Available DNS providers. All variants exist regardless of feature flags -
/// we check at runtime if the selected provider is actually compiled in.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Providers {
    Porkbun,
    Dummy,
    Hickory,
}

impl std::fmt::Display for Providers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Providers::Porkbun => write!(f, "porkbun"),
            Providers::Dummy => write!(f, "dummy"),
            Providers::Hickory => write!(f, "hickory"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    provider: Option<Providers>,
    #[cfg(feature = "porkbun")]
    porkbun: Option<<Porkbun as Interface>::Config>,
    #[cfg(not(feature = "porkbun"))]
    porkbun: Option<Disabled>,
    #[cfg(feature = "dummy")]
    dummy: Option<<Dummy as Interface>::Config>,
    #[cfg(not(feature = "dummy"))]
    dummy: Option<Disabled>,
    #[cfg(feature = "hickory")]
    hickory: Option<<Hickory as Interface>::Config>,
    #[cfg(not(feature = "hickory"))]
    hickory: Option<Disabled>,
}

impl Config {
    /// Warns about provider configs that are present but won't be validated
    /// because the feature isn't compiled in.
    fn warn_uncompiled_providers(&self) {
        #[cfg(not(feature = "porkbun"))]
        if self.porkbun.is_some() {
            log::warn!(
                "[dns.porkbun] config present but 'porkbun' feature not compiled in - config won't be validated"
            );
        }
        #[cfg(not(feature = "dummy"))]
        if self.dummy.is_some() {
            log::warn!(
                "[dns.dummy] config present but 'dummy' feature not compiled in - config won't be validated"
            );
        }
        #[cfg(not(feature = "hickory"))]
        if self.hickory.is_some() {
            log::warn!(
                "[dns.hickory] config present but 'hickory' feature not compiled in - config won't be validated"
            );
        }
    }

    /// Returns which provider is active, checking ALL config sections regardless of feature flags.
    /// This ensures config behavior is consistent no matter which features are compiled in.
    fn active(&self) -> Result<Providers, Error> {
        if let Some(provider) = self.provider {
            return Ok(provider);
        }

        // Count ALL present configs, including disabled ones
        let mut found: Option<Providers> = None;
        let mut count = 0;

        if self.porkbun.is_some() {
            found = Some(Providers::Porkbun);
            count += 1;
        }
        if self.dummy.is_some() {
            found = Some(Providers::Dummy);
            count += 1;
        }
        if self.hickory.is_some() {
            found = Some(Providers::Hickory);
            count += 1;
        }

        match (found, count) {
            (Some(p), 1) => Ok(p),
            (None, 0) => Err(Error::NoConfig),
            _ => Err(Error::CantChoseProvider),
        }
    }
}

pub enum ActiveDns {
    #[cfg(feature = "porkbun")]
    Porkbun(Porkbun),
    #[cfg(feature = "dummy")]
    Dummy(Dummy),
    #[cfg(feature = "hickory")]
    Hickory(Hickory),
}

impl ActiveDns {
    #[cfg(feature = "porkbun")]
    pub fn porkbun(&self) -> Option<&Porkbun> {
        match self {
            ActiveDns::Porkbun(porkbun) => Some(porkbun),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
    #[cfg(feature = "dummy")]
    pub fn dummy(&self) -> Option<&Dummy> {
        match self {
            ActiveDns::Dummy(dummy) => Some(dummy),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
    #[cfg(feature = "hickory")]
    pub fn hickory(&self) -> Option<&Hickory> {
        match self {
            ActiveDns::Hickory(hickory) => Some(hickory),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

pub struct Dns {
    active: ActiveDns,
    #[cfg(feature = "porkbun")]
    porkbun: Option<Porkbun>,
    #[cfg(feature = "dummy")]
    dummy: Option<Dummy>,
    #[cfg(feature = "hickory")]
    hickory: Option<Hickory>,
}

impl Dns {
    #[cfg(feature = "porkbun")]
    pub fn porkbun(&self) -> Result<&Porkbun, Error> {
        self.active
            .porkbun()
            .or(self.porkbun.as_ref())
            .ok_or(Error::MissingConfig("Porkbun"))
    }
    #[cfg(feature = "dummy")]
    pub fn dummy(&self) -> Result<&Dummy, Error> {
        self.active
            .dummy()
            .or(self.dummy.as_ref())
            .ok_or(Error::MissingConfig("Dummy"))
    }
    #[cfg(feature = "hickory")]
    pub fn hickory(&self) -> Result<&Hickory, Error> {
        self.active
            .hickory()
            .or(self.hickory.as_ref())
            .ok_or(Error::MissingConfig("Hickory"))
    }
}

#[derive(Debug)]
pub enum Error {
    #[cfg(feature = "porkbun")]
    Porkbun(<Porkbun as Interface>::Error),
    #[cfg(feature = "dummy")]
    Dummy(<Dummy as Interface>::Error),
    #[cfg(feature = "hickory")]
    Hickory(<Hickory as Interface>::Error),
    MissingConfig(&'static str),
    CantChoseProvider,
    NoConfig,
    /// The selected provider was not compiled into this binary
    ProviderNotCompiled(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "porkbun")]
            Error::Porkbun(err) => write!(f, "{err}"),
            #[cfg(feature = "dummy")]
            Error::Dummy(err) => write!(f, "{err}"),
            #[cfg(feature = "hickory")]
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

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "porkbun")]
            Error::Porkbun(err) => err.source(),
            #[cfg(feature = "dummy")]
            Error::Dummy(err) => err.source(),
            #[cfg(feature = "hickory")]
            Error::Hickory(err) => err.source(),
            Error::MissingConfig(_) => None,
            Error::CantChoseProvider => None,
            Error::NoConfig => None,
            Error::ProviderNotCompiled(_) => None,
        }
    }
}

pub enum Key {
    #[cfg(feature = "porkbun")]
    Porkbun(<Porkbun as Interface>::Key),
    #[cfg(feature = "dummy")]
    Dummy(<Dummy as Interface>::Key),
    #[cfg(feature = "hickory")]
    Hickory(<Hickory as Interface>::Key),
}

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "porkbun")]
            Key::Porkbun(key) => write!(f, "Porkbun:{key}"),
            #[cfg(feature = "dummy")]
            Key::Dummy(key) => write!(f, "Dummy:{key}"),
            #[cfg(feature = "hickory")]
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
            #[cfg(feature = "porkbun")]
            "Porkbun" => Ok(Key::Porkbun(
                key.parse().map_err(|e| format!("Invalid key {key}: {e}"))?,
            )),
            #[cfg(feature = "dummy")]
            "Dummy" => Ok(Key::Dummy(
                key.parse().map_err(|e| format!("Invalid key {key}: {e}"))?,
            )),
            #[cfg(feature = "hickory")]
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
        // Warn about configs that won't be validated
        config.warn_uncompiled_providers();

        let selected = config.active()?;

        // Check if the selected provider is compiled in
        match selected {
            #[cfg(not(feature = "porkbun"))]
            Providers::Porkbun => {
                return Err(Error::ProviderNotCompiled("porkbun".to_string()));
            }
            #[cfg(not(feature = "dummy"))]
            Providers::Dummy => return Err(Error::ProviderNotCompiled("dummy".to_string())),
            #[cfg(not(feature = "hickory"))]
            Providers::Hickory => {
                return Err(Error::ProviderNotCompiled("hickory".to_string()));
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }

        #[cfg(feature = "porkbun")]
        let mut porkbun = config.porkbun.map(Porkbun::new);
        #[cfg(feature = "dummy")]
        let mut dummy = config.dummy.map(Dummy::new);
        #[cfg(feature = "hickory")]
        let mut hickory = config.hickory.map(Hickory::new);

        // set the active dns
        let active = match selected {
            #[cfg(feature = "porkbun")]
            Providers::Porkbun => {
                let porkbun = porkbun
                    .take()
                    .ok_or(Error::MissingConfig("Porkbun"))?
                    .map_err(Error::Porkbun)?;
                ActiveDns::Porkbun(porkbun)
            }
            #[cfg(feature = "dummy")]
            Providers::Dummy => {
                let dummy = dummy
                    .take()
                    .ok_or(Error::MissingConfig("Dummy"))?
                    .map_err(Error::Dummy)?;
                ActiveDns::Dummy(dummy)
            }
            #[cfg(feature = "hickory")]
            Providers::Hickory => {
                let hickory = hickory
                    .take()
                    .ok_or(Error::MissingConfig("Hickory"))?
                    .map_err(Error::Hickory)?;
                ActiveDns::Hickory(hickory)
            }
            #[allow(unreachable_patterns)]
            _ => unreachable!("Provider availability was already checked above"),
        };

        #[cfg(feature = "porkbun")]
        let porkbun = match porkbun {
            Some(Ok(porkbun)) => Some(porkbun),
            Some(Err(e)) => {
                log::error!("Failed to create DNS provider (Porkbun): {e}");
                None
            }
            None => None,
        };
        #[cfg(feature = "dummy")]
        let dummy = match dummy {
            Some(Ok(dummy)) => Some(dummy),
            Some(Err(e)) => {
                log::error!("Failed to create DNS provider (Dummy): {e}");
                None
            }
            None => None,
        };
        #[cfg(feature = "hickory")]
        let hickory = match hickory {
            Some(Ok(hickory)) => Some(hickory),
            Some(Err(e)) => {
                log::error!("Failed to create DNS provider (Hickory): {e}");
                None
            }
            None => None,
        };
        Ok(Dns {
            active,
            #[cfg(feature = "porkbun")]
            porkbun,
            #[cfg(feature = "dummy")]
            dummy,
            #[cfg(feature = "hickory")]
            hickory,
        })
    }

    async fn add_record(
        &self,
        record: fckn_gay_dns_interface::Record,
    ) -> Result<Self::Key, Self::Error> {
        match &self.active {
            #[cfg(feature = "porkbun")]
            ActiveDns::Porkbun(porkbun) => porkbun
                .add_record(record)
                .await
                .map(Key::Porkbun)
                .map_err(Error::Porkbun),
            #[cfg(feature = "dummy")]
            ActiveDns::Dummy(dummy) => dummy
                .add_record(record)
                .await
                .map(Key::Dummy)
                .map_err(Error::Dummy),
            #[cfg(feature = "hickory")]
            ActiveDns::Hickory(hickory) => hickory
                .add_record(record)
                .await
                .map(Key::Hickory)
                .map_err(Error::Hickory),
        }
    }

    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error> {
        match (&self.active, key) {
            #[cfg(feature = "porkbun")]
            (ActiveDns::Porkbun(porkbun), Key::Porkbun(porkbun_key)) => porkbun
                .delete_record(porkbun_key)
                .await
                .map_err(Error::Porkbun),
            #[cfg(feature = "hickory")]
            (ActiveDns::Hickory(hickory), Key::Hickory(hickory_key)) => hickory
                .delete_record(hickory_key)
                .await
                .map_err(Error::Hickory),
            #[cfg(feature = "dummy")]
            (ActiveDns::Dummy(dummy), Key::Dummy(dummy_key)) => {
                dummy.delete_record(dummy_key).await.map_err(Error::Dummy)
            }
            #[allow(unreachable_patterns)]
            _ => panic!("Invalid key type for DNS provider"),
        }
    }

    async fn list_records(
        &self,
    ) -> Result<Vec<(Self::Key, fckn_gay_dns_interface::Record)>, Self::Error> {
        match &self.active {
            #[cfg(feature = "porkbun")]
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
            #[cfg(feature = "dummy")]
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
            #[cfg(feature = "hickory")]
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
            #[cfg(feature = "porkbun")]
            (ActiveDns::Porkbun(porkbun), Key::Porkbun(porkbun_key)) => porkbun
                .update_record(porkbun_key, record)
                .await
                .map_err(Error::Porkbun),
            #[cfg(feature = "hickory")]
            (ActiveDns::Hickory(hickory), Key::Hickory(hickory_key)) => hickory
                .update_record(hickory_key, record)
                .await
                .map_err(Error::Hickory),
            #[cfg(feature = "dummy")]
            (ActiveDns::Dummy(dummy), Key::Dummy(dummy_key)) => dummy
                .update_record(dummy_key, record)
                .await
                .map_err(Error::Dummy),
            #[allow(unreachable_patterns)]
            _ => panic!("Invalid key type for DNS provider"),
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
            entries = []
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
            entries = []
        "#;
        let config: Result<Config, _> = toml::from_str(toml_str);
        assert!(config.is_err(), "Unknown fields should be rejected");
    }

    #[test]
    fn test_ambiguous_config_errors() {
        // Multiple providers without explicit selection should error
        let toml_str = r#"
            [dummy]
            entries = []
            [hickory]
            file_path = "test.log"
            tcp_addr = "127.0.0.1:53"
            udp_addr = "127.0.0.1:53"
            zone_name = "test.gay."
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
            entries = []
            [hickory]
            file_path = "test.log"
            tcp_addr = "127.0.0.1:53"
            udp_addr = "127.0.0.1:53"
            zone_name = "test.gay."
        "#;
        let config: Config = toml::from_str(toml_str).expect("Should parse");
        let result = config.active();
        assert!(
            matches!(result, Ok(Providers::Dummy)),
            "Explicit provider should work: {:?}",
            result
        );
    }

    /// Test that selecting a provider that's not compiled in produces a clear error
    #[test]
    #[cfg(not(feature = "porkbun"))]
    fn test_disabled_provider_error() {
        let toml_str = r#"
            provider = "porkbun"
            [porkbun]
            api_key = "test"
            domain = "fckn.gay"
            secret_key = "test"
        "#;
        let config: Config = toml::from_str(toml_str).expect("Should parse");
        let result = Dns::new(config);
        match result {
            Err(Error::ProviderNotCompiled(provider)) => {
                assert_eq!(provider, "porkbun");
                println!("✅ Got expected error: Provider 'porkbun' not compiled in");
            }
            Err(e) => unreachable!("Wrong error type: {}", e),
            Ok(_) => unreachable!("Should have errored when using disabled provider"),
        }
    }
}
