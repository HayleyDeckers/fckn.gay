mod models;
mod schema;
use diesel::prelude::*;
use fckn_gay_user_database_interface::{
    DnsRecord, DnsRecordId, PasswordHash, UserDatabase, UserEntry, Uuid,
};

#[derive(Debug, serde::Deserialize)]
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
    #[error("Multiple users found")]
    MultipleUsersFound,
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
        let Ok(user) = crate::schema::users::dsl::users
            .filter(crate::schema::users::username.eq(username))
            .first::<models::RawUser>(&mut *self.connection.lock().await)
        else {
            //todo: handle errors
            return false;
        };
        let user = UserEntry::from(user);
        user.is_valid(username, password)
    }

    async fn validate_and_get_user_id(&self, username: &str, password: &str) -> Option<Uuid> {
        let Ok(user) = crate::schema::users::dsl::users
            .filter(crate::schema::users::username.eq(username))
            .first::<models::RawUser>(&mut *self.connection.lock().await)
        else {
            return None;
        };
        let user = UserEntry::from(user);
        if user.is_valid(username, password) {
            Some(user.id)
        } else {
            None
        }
    }
    // this should just be part of add user with a specific error shared between impls
    async fn is_available(&self, username: &str) -> bool {
        match crate::schema::users::dsl::users
            .count()
            .filter(crate::schema::users::username.eq(username))
            .first::<i64>(&mut *self.connection.lock().await)
        {
            Ok(0) => true,
            Ok(_) => false,
            Err(e) => {
                log::error!("Error checking username availability: {e}");
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
        // todo: test this errors correctly on duplicate username
        diesel::insert_into(users)
            .values(new_user)
            .execute(&mut *self.connection.lock().await)?;
        Ok(id)
    }

    async fn activate_user(
        &self,
        uuid: fckn_gay_user_database_interface::Uuid,
    ) -> Result<(), Self::Error> {
        use self::schema::users::dsl::{state, users};
        let bytes = uuid.to_bytes_le();
        let updated = diesel::update(
            users.filter(schema::users::id.eq(&bytes).and(schema::users::state.eq(0))),
        )
        .set(state.eq(1)) // 1 is active
        .execute(&mut *self.connection.lock().await)?;
        if updated == 0 {
            Err(Error::Other("User not found or not pending".to_string()))
        } else {
            Ok(())
        }
    }

    //todo: should be per uuid probably
    async fn delete_user(&self, username: &str) -> Result<(), Self::Error> {
        use self::schema::users::dsl::users;
        diesel::delete(users.filter(schema::users::username.eq(username)))
            .execute(&mut *self.connection.lock().await)?;
        Ok(())
    }

    // DNS record management methods
    async fn add_dns_record(
        &self,
        user_id: fckn_gay_user_database_interface::Uuid,
        record: DnsRecord,
        provider_key: String,
    ) -> Result<DnsRecordId, Self::Error> {
        use self::schema::dns_records::dsl::dns_records;
        let record_id = DnsRecordId::new();
        let new_record =
            models::NewDnsRecord::from_interface(record_id, user_id, &record, &provider_key);

        let mut conn = self.connection.lock().await;
        diesel::insert_into(dns_records)
            .values(new_record)
            .execute(&mut *conn)?;
        Ok(record_id)
    }

    async fn get_user_dns_records(
        &self,
        user_id: fckn_gay_user_database_interface::Uuid,
    ) -> Result<Vec<fckn_gay_user_database_interface::DatabaseDnsRecord>, Self::Error> {
        use self::schema::dns_records::dsl::dns_records;
        let mut conn = self.connection.lock().await;
        let user_bytes = user_id.to_bytes_le();
        let raw_records = dns_records
            .filter(schema::dns_records::user_id.eq(&user_bytes))
            .load::<models::RawDnsRecord>(&mut *conn)?;

        Ok(raw_records
            .into_iter()
            .map(|raw| {
                let record_id = DnsRecordId::from(raw.clone());
                let record = DnsRecord::from(raw.clone());
                fckn_gay_user_database_interface::DatabaseDnsRecord {
                    id: record_id,
                    provider_key: raw.provider_key,
                    record,
                }
            })
            .collect())
    }

    async fn update_dns_record(
        &self,
        user_id: fckn_gay_user_database_interface::Uuid,
        record_id: DnsRecordId,
        record: DnsRecord,
    ) -> Result<(), Self::Error> {
        use self::schema::dns_records::dsl::{
            content, dns_records, name, priority, record_type, ttl_seconds,
        };
        let user_bytes = user_id.to_bytes_le();
        let record_bytes = record_id.0.to_bytes_le();

        // Update with ownership check in a single query
        let updated = diesel::update(
            dns_records.filter(
                schema::dns_records::id
                    .eq(&record_bytes)
                    .and(schema::dns_records::user_id.eq(&user_bytes)),
            ),
        )
        .set((
            name.eq(&record.name),
            record_type.eq(i32::from(record.record_type)),
            content.eq(&record.content),
            ttl_seconds.eq(record.ttl_seconds as i32),
            priority.eq(record.priority.map(|p| p as i32)),
        ))
        .execute(&mut *self.connection.lock().await)?;

        if updated == 0 {
            return Err(Error::Other(
                "Record not found or user does not own it".to_string(),
            ));
        }
        Ok(())
    }

    async fn delete_dns_record(
        &self,
        user_id: fckn_gay_user_database_interface::Uuid,
        record_id: DnsRecordId,
    ) -> Result<(), Self::Error> {
        use self::schema::dns_records::dsl::dns_records;
        let user_bytes = user_id.to_bytes_le();
        let record_bytes = record_id.0.to_bytes_le();

        // Delete with ownership check in a single query
        let deleted = diesel::delete(
            dns_records.filter(
                schema::dns_records::id
                    .eq(&record_bytes)
                    .and(schema::dns_records::user_id.eq(&user_bytes)),
            ),
        )
        .execute(&mut *self.connection.lock().await)?;

        if deleted == 0 {
            return Err(Error::Other(
                "Record not found or user does not own it".to_string(),
            ));
        }
        Ok(())
    }

    async fn get_dns_record_provider_key(
        &self,
        user_id: fckn_gay_user_database_interface::Uuid,
        record_id: DnsRecordId,
    ) -> Result<String, Self::Error> {
        use self::schema::dns_records::dsl::dns_records;
        let user_bytes = user_id.to_bytes_le();
        let record_bytes = record_id.0.to_bytes_le();

        let record = dns_records
            .filter(
                schema::dns_records::id
                    .eq(&record_bytes)
                    .and(schema::dns_records::user_id.eq(&user_bytes)),
            )
            .select(schema::dns_records::provider_key)
            .first::<String>(&mut *self.connection.lock().await)?;

        Ok(record)
    }

    async fn update_user_password(
        &self,
        user_id: Uuid,
        password: PasswordHash,
    ) -> Result<(), Self::Error> {
        use self::schema::users::dsl::{password_hash, users};
        let user_bytes = user_id.to_bytes_le();
        let updated = diesel::update(users.filter(schema::users::id.eq(&user_bytes)))
            .set(password_hash.eq(password.into_string()))
            .execute(&mut *self.connection.lock().await)?;
        if updated == 0 {
            Err(Error::Other(
                "User not found, can't update password".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    async fn get_user_by_username_or_email(
        &self,
        username_or_email: &str,
    ) -> Result<Option<UserEntry>, Self::Error> {
        use self::schema::users::dsl::users;
        let mut conn = self.connection.lock().await;
        let user = users
            .filter(
                schema::users::username
                    .eq(username_or_email)
                    .or(schema::users::email.eq(username_or_email)),
            )
            .load::<models::RawUser>(&mut *conn)?;
        if user.len() > 1 {
            //this should never happen, usernames and emails should be unique
            // and a valid email should never be a valid username
            return Err(Error::MultipleUsersFound);
        }
        Ok(user.into_iter().next().map(UserEntry::from))
    }
}
