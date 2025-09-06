use std::collections::{HashMap, HashSet};

use fckn_gay_user_database_interface::{UserDatabase, UserEntry, UserState, Uuid};

/// a simple user database parsed directly from the configuration file.
///
/// does not support persisting new users to file
pub struct Database {
    users: tokio::sync::Mutex<Vec<UserEntry>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Config(HashMap<String, String>);

#[derive(Debug)]
pub enum Error {
    UserExists,
    UserNotFound,
}

impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UserExists => write!(f, "user already exists"),
            Error::UserNotFound => write!(f, "user not found"),
        }
    }
}

impl UserDatabase for Database {
    /// Configuration type for the UserDatabase.
    type Config = Config;
    type Error = Error;

    /// Creates a new instance of the UserDatabase with the given configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        let users = config
            .0
            .into_iter()
            .map(|(username, password)| UserEntry {
                id: Uuid::new_v4(),
                username: username.clone(),
                password,
                email: format!("\"{username}\"@example.com"),
                state: UserState::Active,
                created_at: std::time::SystemTime::now(),
                last_login: None,
            })
            .collect();
        Ok(Database {
            users: tokio::sync::Mutex::new(users),
        })
    }

    /// Checks if a user exists in the database with the given username and password.
    async fn is_valid(&self, username: &str, password: &str) -> bool {
        self.users
            .lock()
            .await
            .iter()
            .any(|user| user.is_valid(username, password))
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
        lock.push(UserEntry {
            id: user_id,
            username: username.to_string(),
            password: password.to_string(),
            email: email.to_string(),
            state: UserState::Pending,
            created_at: std::time::SystemTime::now(),
            last_login: None,
        });

        Ok(user_id)
    }

    async fn activate_user(&self, uuid: Uuid) -> Result<(), Self::Error> {
        let mut lock = self.users.lock().await;
        if let Some(user) = lock.iter_mut().find(|user| user.id == uuid) {
            if user.state == UserState::Pending {
                user.state = UserState::Active;
                return Ok(());
            }
        }
        Err(Error::UserNotFound)
    }
}
