use std::collections::HashSet;

use fckn_gay_user_database_interface::UserDatabase;

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

    /// Checks if a user is available for registration.
    async fn is_available(&self, username: &str) -> bool {
        !self.users.iter().any(|(user, _)| user == username)
    }
}
