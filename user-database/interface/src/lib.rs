pub use uuid::Uuid;

#[derive(PartialEq, Eq)]
pub enum UserState {
    Pending,
    Active,
    Inactive,
    Banned,
}

// uuid field is also the key for activating an account
pub struct UserEntry {
    pub id: Uuid,
    pub username: String,
    // hashed password
    pub password: String,
    pub email: String,
    pub state: UserState,
    pub created_at: std::time::SystemTime,
    pub last_login: Option<std::time::SystemTime>,
}

impl UserEntry {
    pub fn is_active(&self) -> bool {
        matches!(self.state, UserState::Active)
    }

    // todo(hayley): this is not very secure, not constant time or hashed
    pub fn is_valid(&self, username: &str, password: &str) -> bool {
        self.is_active() && self.password == password && self.username == username
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
