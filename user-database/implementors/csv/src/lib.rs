use std::collections::HashSet;

use fckn_gay_user_database_interface::{DnsRecord, DnsRecordId, UserDatabase, Uuid};

/// a simple user database parsed from a CSV file.
pub struct Database {
    users: HashSet<(String, String)>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Config {
    file: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct User {
    username: String,
    password: String,
}

impl UserDatabase for Database {
    /// Configuration type for the `UserDatabase`.
    type Config = Config;
    type Error = csv::Error;

    /// Creates a new instance of the `UserDatabase` with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        let mut rdr = csv::Reader::from_path(config.file).expect("Failed to open CSV file");
        let users = rdr
            .deserialize()
            .map(|r| r.map(|user: User| (user.username, user.password)))
            .collect::<Result<HashSet<_>, _>>()?;
        Ok(Database { users })
    }

    /// Checks if a user exists in the database with the given username and password.
    async fn is_valid(&self, username: &str, password: &str) -> bool {
        self.users
            .contains(&(username.to_string(), password.to_string()))
    }

    async fn validate_and_get_user_id(&self, username: &str, password: &str) -> Option<Uuid> {
        // CSV implementation doesn't store UUIDs, so we can't return a real UUID
        // This is a limitation of the CSV implementation
        if self
            .users
            .contains(&(username.to_string(), password.to_string()))
        {
            Some(Uuid::new_v4()) // Generate a new UUID each time (not ideal)
        } else {
            None
        }
    }

    /// Checks if a user is available for registration.
    async fn is_available(&self, username: &str) -> bool {
        !self.users.iter().any(|(user, _)| user == username)
    }

    #[expect(unused_variables)]
    async fn add_dns_record(
        &self,
        user_id: Uuid,
        record: DnsRecord,
        provider_key: String,
    ) -> Result<DnsRecordId, Self::Error> {
        // CSV implementation - just return a new ID
        Ok(DnsRecordId::new())
    }

    #[expect(unused_variables)]
    async fn get_user_dns_records(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<fckn_gay_user_database_interface::DatabaseDnsRecord>, Self::Error> {
        // CSV implementation - return empty list
        Ok(vec![])
    }

    #[expect(unused_variables)]
    async fn update_dns_record(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
        record: DnsRecord,
    ) -> Result<(), Self::Error> {
        // CSV implementation - do nothing
        Ok(())
    }

    #[expect(unused_variables)]
    async fn delete_dns_record(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
    ) -> Result<(), Self::Error> {
        // CSV implementation - do nothing
        Ok(())
    }

    #[expect(unused_variables)]
    async fn get_dns_record_provider_key(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
    ) -> Result<String, Self::Error> {
        // CSV implementation - return a dummy key
        Ok(format!("csv_key_{}", record_id.0))
    }
}
