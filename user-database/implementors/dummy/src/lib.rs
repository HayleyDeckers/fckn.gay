use std::collections::HashMap;

use fckn_gay_user_database_interface::{
    DnsRecord, DnsRecordId, PasswordHash, UserDatabase, UserEntry, UserState, Uuid,
};

/// a simple user database parsed directly from the configuration file.
///
/// does not support persisting new users to file
pub struct Database {
    users: tokio::sync::Mutex<Vec<UserEntry>>,
    records: tokio::sync::Mutex<HashMap<(Uuid, DnsRecordId), (String, DnsRecord)>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Config(HashMap<String, String>);

#[derive(Debug)]
pub enum Error {
    UserExists,
    UserNotFound,
    RecordNotFound,
}

impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UserExists => write!(f, "user already exists"),
            Error::UserNotFound => write!(f, "user not found"),
            Error::RecordNotFound => write!(f, "record not found"),
        }
    }
}

impl UserDatabase for Database {
    /// Configuration type for the `UserDatabase`.
    type Config = Config;
    type Error = Error;

    /// Creates a new instance of the `UserDatabase` with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        let users = config
            .0
            .into_iter()
            .map(|(username, password)| {
                let password_hash = PasswordHash::new(&password);
                UserEntry {
                    id: Uuid::new_v4(),
                    username: username.clone(),
                    password_hash,
                    email: format!("\"{username}\"@example.com"),
                    state: UserState::Active,
                    created_at: chrono::Utc::now().naive_utc(),
                    last_login: None,
                }
            })
            .collect();
        Ok(Database {
            users: tokio::sync::Mutex::new(users),
            records: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Checks if a user exists in the database with the given username and password.
    async fn is_valid(&self, username: &str, password: &str) -> bool {
        //relies on user.is_valid short-circuiting the check on username and active state before hashing for performance reasons
        self.users
            .lock()
            .await
            .iter()
            .any(|user| user.is_valid(username, password))
    }

    async fn validate_and_get_user_id(&self, username: &str, password: &str) -> Option<Uuid> {
        self.users
            .lock()
            .await
            .iter()
            .find(|user| user.is_valid(username, password))
            .map(|user| user.id)
    }

    /// Checks if a user is available for registration.
    async fn is_available(&self, username: &str) -> bool {
        !self
            .users
            .lock()
            .await
            .iter()
            .any(|user| user.username == username)
    }
    async fn add_user(
        &self,
        username: &str,
        password: &str,
        email: &str,
    ) -> Result<Uuid, Self::Error> {
        /* todo(hayley): we need some kind of common error per interface so we can match on
          logic errors like "user exists" or "user not found" which is shared among all implementations
        */
        let mut lock = self.users.lock().await;
        if lock.iter().any(|user| user.username == username) {
            return Err(Error::UserExists);
        }
        let user_id = Uuid::new_v4();
        let password_hash = PasswordHash::new(password);
        lock.push(UserEntry {
            id: user_id,
            username: username.to_string(),
            password_hash,
            email: email.to_string(),
            state: UserState::Pending,
            created_at: chrono::Utc::now().naive_utc(),
            last_login: None,
        });

        Ok(user_id)
    }

    async fn activate_user(&self, uuid: Uuid) -> Result<(), Self::Error> {
        let mut lock = self.users.lock().await;
        if let Some(user) = lock.iter_mut().find(|user| user.id == uuid)
            && user.state == UserState::Pending
        {
            user.state = UserState::Active;
            return Ok(());
        }
        Err(Error::UserNotFound)
    }

    #[allow(unused_variables)]
    async fn get_user_dns_records(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<fckn_gay_user_database_interface::DatabaseDnsRecord>, Self::Error> {
        let lock = self.records.lock().await;
        let records = lock
            .iter()
            .filter(|((x, _), _)| *x == user_id)
            .map(|x| fckn_gay_user_database_interface::DatabaseDnsRecord {
                id: x.0.1.clone(),
                provider_key: x.1.0.clone(),
                record: x.1.1.clone(),
            })
            .collect();
        Ok(records)
    }

    #[allow(unused_variables)]
    async fn add_dns_record(
        &self,
        user_id: Uuid,
        record: DnsRecord,
        provider_key: String,
    ) -> Result<DnsRecordId, Self::Error> {
        let record_id = DnsRecordId(uuid::Uuid::new_v4());
        let mut lock = self.records.lock().await;
        lock.insert((user_id, record_id.clone()), (provider_key, record));
        Ok(record_id)
    }

    #[allow(unused_variables)]
    async fn update_dns_record(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
        record: DnsRecord,
    ) -> Result<(), Self::Error> {
        let mut lock = self.records.lock().await;
        let Some(entry) = lock.get(&(user_id, record_id.clone())).cloned() else {
            return Err(Error::RecordNotFound);
        };
        lock.insert((user_id, record_id.clone()), (entry.0, record));
        Ok(())
    }

    #[allow(unused_variables)]
    async fn delete_dns_record(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
    ) -> Result<(), Self::Error> {
        let mut lock = self.records.lock().await;
        let Some(entry) = lock.remove(&(user_id, record_id.clone())) else {
            return Err(Error::RecordNotFound);
        };
        Ok(())
    }

    async fn get_dns_record_provider_key(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
    ) -> Result<String, Self::Error> {
        let lock = self.records.lock().await;
        let Some(entry) = lock.get(&(user_id, record_id.clone())).cloned() else {
            return Err(Error::RecordNotFound);
        };
        Ok(format!("dummy_key_{}", entry.0))
    }

    async fn update_user_password(
        &self,
        user_id: Uuid,
        password: PasswordHash,
    ) -> Result<(), Self::Error> {
        let mut lock = self.users.lock().await;
        if let Some(user) = lock.iter_mut().find(|u| u.id == user_id) {
            user.password_hash = password;
            Ok(())
        } else {
            Err(Error::UserNotFound)
        }
    }

    async fn get_user_by_username_or_email(
        &self,
        username_or_email: &str,
    ) -> Result<Option<UserEntry>, Self::Error> {
        Ok(self
            .users
            .lock()
            .await
            .iter()
            .find(|user| user.username == username_or_email || user.email == username_or_email)
            .cloned())
    }
}
