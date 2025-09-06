use fckn_gay_dns_interface::{Dns, Record};
use serde::Deserialize;

/// configuration for the Porkbun DNS provider.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// The domain name to manage with this Porkbun clien.
    pub entries: Vec<Record>,
}

/// A DNS provider implementation using Porkbun.
/// This struct holds the client for interacting with the Porkbun API.
pub struct DummyDns {
    entries: tokio::sync::Mutex<Vec<Option<Record>>>,
}

impl Dns for DummyDns {
    type Config = Config;
    type Error = std::convert::Infallible;
    type Key = usize;

    /// Creates a new instance of the DNS provider with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        let Config { entries } = config;
        let entries = tokio::sync::Mutex::new(entries.into_iter().map(Some).collect());
        Ok(DummyDns { entries })
    }

    /// Adds a DNS record to the provider.
    async fn add_record(&self, record: Record) -> Result<Self::Key, Self::Error> {
        let mut entries = self.entries.lock().await;
        if let Some((i, _)) = entries
            .iter()
            .filter_map(|r| r.as_ref())
            .enumerate()
            .find(|(_, r)| r.name == record.name && r.record_type == record.record_type)
        {
            return Ok(i);
        }
        if let Some((i, r)) = entries.iter_mut().enumerate().find(|(_, r)| r.is_some()) {
            *r = Some(record);
            return Ok(i);
        }
        entries.push(Some(record));
        Ok(entries.len() - 1)
    }

    /// Deletes a DNS record from the provider.
    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error> {
        let mut entries = self.entries.lock().await;
        //todo: error if key is out of bounds
        entries.get_mut(key).map(|r| *r = None);
        Ok(())
    }

    /// Lists all DNS records
    async fn list_records(&self) -> Result<Vec<(Self::Key, Record)>, Self::Error> {
        let entries = self.entries.lock().await;
        Ok(entries
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.as_ref().map(|r| (i, r.clone())))
            .collect())
    }
}
