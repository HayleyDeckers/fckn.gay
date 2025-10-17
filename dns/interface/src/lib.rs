use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize};

/// Type of the DNS record.
// reduced functionality to match the current state of the project
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Copy)]
pub enum RecordType {
    A,
    MX,
    CNAME,
    ALIAS, //note: not standard, might not be settable on all providers
    TXT,
    NS,
    AAAA,
    SRV,
    TLSA,
    CAA,
    HTTPS,
    SVCB,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Record {
    pub name: String,
    pub record_type: RecordType,
    pub content: String,
    #[serde(deserialize_with = "deserialize_ttl")]
    pub ttl_seconds: u32, // Time to live in seconds
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub priority: Option<u16>,
}

// deserialize ttl from null to the default
fn deserialize_ttl<'de, D: Deserializer<'de>>(deserialize: D) -> Result<u32, D::Error> {
    const MIN_TTL_SECONDS: u32 = 300;
    let opt = Option::deserialize(deserialize)?;
    if let Some(ttl) = opt
        && ttl >= MIN_TTL_SECONDS
    {
        Ok(ttl)
    } else {
        Ok(MIN_TTL_SECONDS)
    }
}

impl Display for RecordType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RecordType::A => "A",
            RecordType::AAAA => "AAAA",
            RecordType::CNAME => "CNAME",
            RecordType::MX => "MX",
            RecordType::NS => "NS",
            RecordType::SRV => "SRV",
            RecordType::TXT => "TXT",
            RecordType::ALIAS => "ALIAS",
            RecordType::CAA => "CAA",
            RecordType::HTTPS => "HTTPS",
            RecordType::SVCB => "SVCB",
            RecordType::TLSA => "TLSA",
        };
        write!(f, "{s}")
    }
}

impl FromStr for RecordType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "A" => Ok(RecordType::A),
            "AAAA" => Ok(RecordType::AAAA),
            "CNAME" => Ok(RecordType::CNAME),
            "MX" => Ok(RecordType::MX),
            "NS" => Ok(RecordType::NS),
            "SRV" => Ok(RecordType::SRV),
            "TXT" => Ok(RecordType::TXT),
            "ALIAS" => Ok(RecordType::ALIAS),
            "CAA" => Ok(RecordType::CAA),
            "HTTPS" => Ok(RecordType::HTTPS),
            "SVCB" => Ok(RecordType::SVCB),
            "TLSA" => Ok(RecordType::TLSA),
            _ => Err(format!("Unknown record type: {s}")),
        }
    }
}

impl Display for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.name, self.record_type, self.ttl_seconds, self.content
        )?;
        if let Some(priority) = self.priority {
            write!(f, " {priority}")
        } else {
            Ok(())
        }
    }
}

impl FromStr for Record {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((name, rest)) = s.split_once(' ') else {
            return Err(format!("invalid record string: {s}"));
        };
        let name = name.to_string();

        let Some((record, rest)) = rest.split_once(' ') else {
            return Err(format!("invalid record string: {s}"));
        };
        let record_type = record.parse()?;
        let Some((ttl, rest)) = rest.split_once(' ') else {
            return Err(format!("invalid record string: {s}"));
        };
        let ttl_seconds = ttl
            .parse()
            .map_err(|e: std::num::ParseIntError| format!("Invalid TTL seconds: {e}"))?;
        if record_type == RecordType::MX {
            let Some((content, priority)) = rest.rsplit_once(' ') else {
                return Err(format!("Invalid MX record, missing priority: {s}"));
            };
            let priority = priority
                .parse()
                .map_err(|e| format!("Invalid priority: {e}"))?;
            let content = content.to_string();
            Ok(Record {
                name,
                record_type,
                content,
                ttl_seconds,
                priority: Some(priority),
            })
        } else {
            let content = rest.to_string();
            Ok(Record {
                name,
                record_type,
                content,
                ttl_seconds,
                priority: None,
            })
        }
    }
}

/// a trait for setting up a DNS provider
pub trait Dns {
    type Config: serde::de::DeserializeOwned;
    type Error: std::error::Error + Send + Sync + 'static;
    type Key: serde::Serialize + serde::de::DeserializeOwned;

    /// Creates a new instance of the DNS provider with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error>
    where
        Self: Sized;

    /// Adds a DNS record to the provider.
    ///
    /// # Arguments
    ///
    /// * `record` - The DNS record to add.
    ///
    /// # Returns
    ///
    /// A result indicating success or failure.
    fn add_record(&self, record: Record) -> impl Future<Output = Result<Self::Key, Self::Error>>;

    /// Deletes a DNS record from the provider using its key.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the DNS record to delete.
    ///
    /// # Returns
    ///
    /// A result indicating success or failure.
    fn delete_record_by_uuid(
        &self,
        key: Self::Key,
    ) -> impl Future<Output = Result<(), Self::Error>>;

    /// Deletes DNS records from the provider by matching the full record.
    ///
    /// # Arguments
    ///
    /// * `record` - The full record to match and delete.
    ///
    /// # Returns
    ///
    /// A result indicating success or failure.
    fn delete_record_by_match(
        &self,
        record: Record,
    ) -> impl Future<Output = Result<(), Self::Error>>;

    /// Lists all DNS records for a domain.
    ///
    /// # Arguments
    ///
    /// * `domain` - The domain for which to list records.
    ///
    /// # Returns
    ///
    /// A vector of DNS records or an error message.
    fn list_records(&self) -> impl Future<Output = Result<Vec<(Self::Key, Record)>, Self::Error>>;
}
