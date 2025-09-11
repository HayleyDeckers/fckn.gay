pub use uuid::Uuid;

#[derive(Debug)]
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

#[derive(PartialEq, Eq, Debug)]
pub enum UserState {
    Pending,
    Active,
    Inactive,
    Banned,
}

// uuid field is also the key for activating an account
#[derive(Debug)]
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

// alternatively to this, would it be better to use a raw trait for "add entry"
// and a "add user" wrapper that is "check and add user"
pub enum Error<E> {
    UserExists,
    UserNotFound,
    ImplementorError(E),
}

pub trait UserDatabase {
    /// Configuration type for the UserDatabase.
    type Config: serde::de::DeserializeOwned;
    /// Error type for the UserDatabase.
    type Error: std::error::Error + Send + Sync + 'static;
    /// Creates a new instance of the UserDatabase with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error>
    where
        Self: Sized;
    /// Checks if a user exists in the database with the given username and password.
    fn is_valid(&self, username: &str, password: &str) -> impl Future<Output = bool>;

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
}
