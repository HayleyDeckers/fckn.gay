use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use fckn_gay_dns::{Dns, Interface as DnsInterface, Key, Record};
use tokio::sync::Mutex;

/// A service layer that wraps the DNS interface and handles user ownership
/// This eliminates the need for repetitive ownership checking in API endpoints
pub struct DnsService {
    /// The underlying DNS provider
    dns: Arc<Dns>,
    /// Maps user IDs to their DNS record keys
    /// This allows us to quickly check ownership without scanning all records
    user_records: Arc<Mutex<HashMap<String, Vec<Key>>>>,
}

impl DnsService {
    /// Creates a new DnsService
    pub fn new(dns: Arc<Dns>) -> Self {
        Self {
            dns,
            user_records: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get all DNS records for a specific user
    pub async fn get_user_records(&self, user_id: &str) -> Result<Vec<(Key, Record)>> {
        let user_records = self.user_records.lock().await;
        let user_keys = user_records.get(user_id).cloned().unwrap_or_default();

        let all_records = self.dns.list_records().await?;

        // Filter to only records owned by this user
        let user_records: Vec<_> = all_records
            .into_iter()
            .filter(|(key, _)| user_keys.contains(key))
            .collect();

        Ok(user_records)
    }

    /// Add a new DNS record for a user
    pub async fn add_user_record(&self, user_id: &str, record: Record) -> Result<Key> {
        // Add the record to the DNS provider
        let key = self.dns.add_record(record).await?;

        // Track ownership
        let mut user_records = self.user_records.lock().await;
        user_records
            .entry(user_id.to_string())
            .or_insert_with(Vec::new)
            .push(key.clone());

        Ok(key)
    }

    /// Delete a DNS record by key (only if owned by the user)
    pub async fn delete_user_record_by_key(&self, user_id: &str, key: Key) -> Result<()> {
        // Check ownership first
        let mut user_records = self.user_records.lock().await;
        if let Some(user_keys) = user_records.get_mut(user_id)
            && user_keys.contains(&key)
        {
            // Remove from our tracking
            user_keys.retain(|k| *k != key);
            // Delete from DNS provider
            self.dns.delete_record_by_uuid(key).await?;
            return Ok(());
        }

        Err(anyhow::anyhow!("Record not found or not owned by user"))
    }

    /// Delete a DNS record by matching the full record (only if owned by the user)
    pub async fn delete_user_record_by_match(&self, user_id: &str, record: Record) -> Result<()> {
        // Get user's records to find the matching one
        let user_records = self.user_records.lock().await;
        let user_keys = user_records.get(user_id).cloned().unwrap_or_default();

        // Get all records and find the matching one
        let all_records = self.dns.list_records().await?;
        let matching_key = all_records
            .iter()
            .find(|(key, r)| {
                user_keys.contains(key)
                    && r.name == record.name
                    && r.record_type == record.record_type
                    && r.content == record.content
                    && r.ttl_seconds == record.ttl_seconds
                    && r.priority == record.priority
            })
            .map(|(key, _)| key.clone());

        if let Some(key) = matching_key {
            // Remove from our tracking
            let mut user_records = self.user_records.lock().await;
            if let Some(user_keys) = user_records.get_mut(user_id) {
                user_keys.retain(|k| *k != key);
            }
            // Delete from DNS provider
            self.dns.delete_record_by_match(record).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Record not found or not owned by user"))
        }
    }

    /// Check if a user owns a specific key
    pub async fn user_owns_key(&self, user_id: &str, key: &Key) -> bool {
        let user_records = self.user_records.lock().await;
        user_records
            .get(user_id)
            .is_some_and(|keys| keys.contains(key))
    }
}
