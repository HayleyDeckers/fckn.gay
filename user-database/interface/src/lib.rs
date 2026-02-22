use serde::{Deserialize, Serialize};
pub use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PasswordHash(String);
impl PasswordHash {
    pub fn new(password: &str) -> Self {
        Self(password_auth::generate_hash(password.as_bytes()))
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub fn validate(&self, password: &str) -> bool {
        //todo: this can error if the hash is malformed. Shouldn't happen but we might want to log it.
        password_auth::verify_password(password.as_bytes(), self.0.as_str()).is_ok()
    }

    //todo: validate that the raw hash is valid
    pub fn from_raw(raw: String) -> Self {
        Self(raw)
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum UserState {
    Pending,
    Active,
    Inactive,
    Banned,
}

// uuid field is also the key for activating an account
#[derive(Debug, Clone)]
pub struct UserEntry {
    pub id: Uuid,
    pub username: String,
    pub password_hash: PasswordHash,
    pub email: String,
    pub state: UserState,
    pub created_at: chrono::NaiveDateTime,
    pub last_login: Option<chrono::NaiveDateTime>,
}

impl UserEntry {
    pub fn is_active(&self) -> bool {
        matches!(self.state, UserState::Active)
    }

    // while the validation of the password hash should be constant-time, the other checks and fetching are not
    // so there might be some ways to time attacks here, but it should be limited to enumeration of users
    // and given the public nature of the service (dns entries) that is acceptable.
    // (and also we should have guardrails agains enumeration attacks elsewhere)
    pub fn is_valid(&self, username: &str, password: &str) -> bool {
        // order is important here for short-circuiting to avoid unnecessary hashing
        self.is_active() && self.username == username && self.password_hash.validate(password)
    }
}

// DNS record types for user-database
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsRecordId(pub Uuid);

impl DnsRecordId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl From<Uuid> for DnsRecordId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<DnsRecordId> for Uuid {
    fn from(id: DnsRecordId) -> Self {
        id.0
    }
}

// Re-export DNS types for convenience
pub use fckn_gay_dns_interface::{Record as DnsRecord, RecordType as DnsRecordType};

/// A DNS record as stored in the database, including metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseDnsRecord {
    pub id: DnsRecordId,
    pub provider_key: String,
    pub record: DnsRecord,
}

// alternatively to this, would it be better to use a raw trait for "add entry"
// and a "add user" wrapper that is "check and add user"
pub enum Error<E> {
    UserExists,
    UserNotFound,
    RecordNotFound,
    ImplementorError(E),
}

pub trait UserDatabase {
    /// Configuration type for the `UserDatabase`.
    type Config: serde::de::DeserializeOwned;
    /// Error type for the `UserDatabase`.
    type Error: std::error::Error + Send + Sync + 'static;
    /// Creates a new instance of the `UserDatabase` with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error>
    where
        Self: Sized;
    /// Checks if a user exists in the database with the given username and password.
    fn is_valid(&self, username: &str, password: &str) -> impl Future<Output = bool>;

    /// Validates user credentials and returns the user ID if valid.
    fn validate_and_get_user_id(
        &self,
        username: &str,
        password: &str,
    ) -> impl Future<Output = Option<Uuid>>;

    /// Checks if a user is available for registration.
    fn is_available(&self, username: &str) -> impl Future<Output = bool>;
    #[allow(unused_variables)]
    /// Adds a new user to the database with the given username and password.
    fn add_user(
        &self,
        username: &str,
        password: &str,
        email: &str,
    ) -> impl Future<Output = Result<Uuid, Self::Error>> {
        async { todo!("add_user is not implemented") }
    }

    #[allow(unused_variables)]
    /// Deletes a user from the database with the given username.
    fn delete_user(&self, username: &str) -> impl Future<Output = Result<(), Self::Error>> {
        async { todo!("delete_user is not implemented") }
    }

    #[allow(unused_variables)]
    fn activate_user(&self, uuid: Uuid) -> impl Future<Output = Result<(), Self::Error>> {
        async { todo!("activate_user is not implemented") }
    }

    #[allow(unused_variables)]
    fn update_user_password(
        &self,
        user_id: Uuid,
        password: PasswordHash,
    ) -> impl Future<Output = Result<(), Self::Error>> {
        async { todo!("update_user_password is not implemented") }
    }

    #[allow(unused_variables)]
    fn get_user_by_username_or_email(
        &self,
        username_or_email: &str,
    ) -> impl Future<Output = Result<Option<UserEntry>, Self::Error>> {
        async { todo!("get_user_by_username_or_email is not implemented") }
    }

    // DNS record management methods
    #[allow(unused_variables)]
    /// Adds a DNS record for the given user with the provider key.
    fn add_dns_record(
        &self,
        user_id: Uuid,
        record: DnsRecord,
        provider_key: String,
    ) -> impl Future<Output = Result<DnsRecordId, Self::Error>> {
        async { todo!("add_dns_record is not implemented") }
    }

    #[allow(unused_variables)]
    /// Gets all DNS records for the given user.
    fn get_user_dns_records(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<DatabaseDnsRecord>, Self::Error>> {
        async { todo!("get_user_dns_records is not implemented") }
    }

    #[allow(unused_variables)]
    /// Updates a DNS record (verifies user ownership).
    fn update_dns_record(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
        record: DnsRecord,
    ) -> impl Future<Output = Result<(), Self::Error>> {
        async { todo!("update_dns_record is not implemented") }
    }

    #[allow(unused_variables)]
    /// Deletes a DNS record (verifies user ownership).
    fn delete_dns_record(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
    ) -> impl Future<Output = Result<(), Self::Error>> {
        async { todo!("delete_dns_record is not implemented") }
    }

    #[allow(unused_variables)]
    /// Gets the provider key for a specific DNS record (verifies user ownership).
    fn get_dns_record_provider_key(
        &self,
        user_id: Uuid,
        record_id: DnsRecordId,
    ) -> impl Future<Output = Result<String, Self::Error>> {
        async { todo!("get_dns_record_provider_key is not implemented") }
    }
}
