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

impl From<RecordType> for i32 {
    fn from(record_type: RecordType) -> Self {
        match record_type {
            RecordType::A => 0,
            RecordType::MX => 1,
            RecordType::CNAME => 2,
            RecordType::ALIAS => 3,
            RecordType::TXT => 4,
            RecordType::NS => 5,
            RecordType::AAAA => 6,
            RecordType::SRV => 7,
            RecordType::TLSA => 8,
            RecordType::CAA => 9,
            RecordType::HTTPS => 10,
            RecordType::SVCB => 11,
        }
    }
}

impl TryFrom<i32> for RecordType {
    type Error = i32;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(RecordType::A),
            1 => Ok(RecordType::MX),
            2 => Ok(RecordType::CNAME),
            3 => Ok(RecordType::ALIAS),
            4 => Ok(RecordType::TXT),
            5 => Ok(RecordType::NS),
            6 => Ok(RecordType::AAAA),
            7 => Ok(RecordType::SRV),
            8 => Ok(RecordType::TLSA),
            9 => Ok(RecordType::CAA),
            10 => Ok(RecordType::HTTPS),
            11 => Ok(RecordType::SVCB),
            _ => Err(value),
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
    type Key: Clone;

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

    /// Deletes a DNS record from the provider.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the DNS record to delete.
    /// * `record_type` - The type of the DNS record to delete.
    ///
    /// # Returns
    ///
    /// A result indicating success or failure.
    fn delete_record(&self, key: Self::Key) -> impl Future<Output = Result<(), Self::Error>>;

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

    /// Updates a DNS record.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the DNS record to update.
    /// * `record` - The new DNS record content.
    ///
    /// # Returns
    ///
    /// A result indicating success or failure.
    fn update_record(
        &self,
        key: Self::Key,
        record: Record,
    ) -> impl Future<Output = Result<(), Self::Error>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_type_roundtrip() {
        let all_variants = [
            RecordType::A,
            RecordType::MX,
            RecordType::CNAME,
            RecordType::ALIAS,
            RecordType::TXT,
            RecordType::NS,
            RecordType::AAAA,
            RecordType::SRV,
            RecordType::TLSA,
            RecordType::CAA,
            RecordType::HTTPS,
            RecordType::SVCB,
        ];

        for variant in all_variants {
            let int_value = i32::from(variant);
            let back_to_variant = RecordType::try_from(int_value).expect("roundtrip should work");
            assert_eq!(
                variant, back_to_variant,
                "RecordType {:?} should roundtrip through integer {}",
                variant, int_value
            );
        }
    }

    #[test]
    fn test_invalid_record_type() {
        // 12 should be the next value after the last valid value
        // so always test for that here.
        let invalid_values = [-1, 12, 999, i32::MAX, i32::MIN];

        for invalid_value in invalid_values {
            let result = RecordType::try_from(invalid_value);
            assert!(
                result.is_err(),
                "Invalid value {} should return error",
                invalid_value
            );
            assert_eq!(
                result.unwrap_err(),
                invalid_value,
                "Error should contain the invalid value"
            );
        }
    }
}
