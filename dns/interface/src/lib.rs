use serde::{Deserialize, Serialize};

/// Type of the DNS record.
// reduced functionality to match the current state of the project
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum RecordType {
    A,
    MX,
    CNAME,
    ALIAS,
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
    pub ttl_seconds: u32, // Time to live in seconds
    pub priority: Option<u16>,
}

/// a trait for setting up a DNS provider
pub trait Dns {
    type Config: serde::de::DeserializeOwned;
    type Error: std::error::Error + Send + Sync + 'static;
    type Key;

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
}
