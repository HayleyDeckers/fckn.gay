mod models;
mod schema;
use diesel::prelude::*;
use fckn_gay_user_database_interface::{PasswordHash, UserDatabase, UserEntry};

#[derive(serde::Deserialize)]
pub struct Config {
    pub database_url: String,
}

pub struct Database {
    connection: tokio::sync::Mutex<SqliteConnection>,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Database connection error")]
    ConnectionError(#[from] diesel::ConnectionError),
    #[error("Database Error")]
    DatabaseError(#[from] diesel::result::Error),
    #[error("{0}")]
    Other(String),
}

impl UserDatabase for Database {
    type Config = Config;
    type Error = Error;

    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        let connection =
            SqliteConnection::establish(&config.database_url).map_err(Error::ConnectionError)?;
        Ok(Database {
            connection: tokio::sync::Mutex::new(connection),
        })
    }

    async fn is_valid(&self, username: &str, password: &str) -> bool {
        let mut conn = self.connection.lock().await;
        let Ok(user) = crate::schema::users::dsl::users
            .filter(crate::schema::users::username.eq(username))
            .first::<models::RawUser>(&mut *conn)
        else {
            //todo: handle errors
            return false;
        };
        let user = UserEntry::from(user);
        user.is_valid(username, password)
    }
    // this should just be part of add user with a specific error shared between impls
    async fn is_available(&self, username: &str) -> bool {
        let mut conn = self.connection.lock().await;
        match crate::schema::users::dsl::users
            .count()
            .filter(crate::schema::users::username.eq(username))
            .first::<i64>(&mut *conn)
        {
            Ok(0) => true,
            Ok(_) => false,
            Err(e) => {
                eprintln!("Error checking username availability: {e}");
                true
            }
        }
    }

    // todo: should pass in a hashed password maybe?
    async fn add_user(
        &self,
        username: &str,
        password: &str,
        email: &str,
    ) -> Result<fckn_gay_user_database_interface::Uuid, Self::Error> {
        use self::schema::users::dsl::users;
        let id = fckn_gay_user_database_interface::Uuid::new_v4();
        let password_hash = PasswordHash::new(password).into_string();
        let new_user = models::NewUser {
            id: id.to_bytes_le(),
            username,
            email,
            password_hash: password_hash.as_str(),
        };
        let mut conn = self.connection.lock().await;
        // todo: test this errors correctly on duplicate username
        diesel::insert_into(users)
            .values(new_user)
            .execute(&mut *conn)?;
        Ok(id)
    }

    async fn activate_user(
        &self,
        uuid: fckn_gay_user_database_interface::Uuid,
    ) -> Result<(), Self::Error> {
        use self::schema::users::dsl::{state, users};
        let mut conn = self.connection.lock().await;
        let bytes = uuid.to_bytes_le();
        let updated = diesel::update(
            users.filter(schema::users::id.eq(&bytes).and(schema::users::state.eq(0))),
        )
        .set(state.eq(1)) // 1 is active
        .execute(&mut *conn)?;
        if updated == 0 {
            Err(Error::Other("User not found or not pending".to_string()))
        } else {
            Ok(())
        }
    }

    //todo: should be per uuid probably
    async fn delete_user(&self, username: &str) -> Result<(), Self::Error> {
        use self::schema::users::dsl::users;
        let mut conn = self.connection.lock().await;
        diesel::delete(users.filter(schema::users::username.eq(username))).execute(&mut *conn)?;
        Ok(())
    }
}
