use diesel::prelude::*;
use fckn_gay_user_database_interface::{PasswordHash, UserState, Uuid};

#[derive(Queryable, Selectable, Debug)]
#[diesel(table_name = crate::schema::users)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct RawUser {
    // order is important here and must match the schema exactly
    // can we somehow force it to be per name?
    pub id: Vec<u8>,
    pub username: String,
    pub password_hash: String,
    pub email: String,
    pub state: i32,
    pub created_at: chrono::NaiveDateTime,
    pub last_login: Option<chrono::NaiveDateTime>,
}

impl From<RawUser> for fckn_gay_user_database_interface::UserEntry {
    fn from(raw: RawUser) -> Self {
        let RawUser {
            id,
            username,
            email,
            password_hash,
            state,
            created_at,
            last_login,
        } = raw;
        let id = Uuid::from_slice_le(&id).unwrap();
        let password_hash = PasswordHash::from_raw(password_hash);
        let state = match state {
            0 => UserState::Pending,
            1 => UserState::Active,
            2 => UserState::Inactive,
            3 => UserState::Banned,
            _ => panic!("invalid user state in database"),
        };
        Self {
            id,
            username,
            email,
            password_hash,
            state,
            created_at,
            last_login,
        }
    }
}

#[derive(Insertable, Debug)]
#[diesel(table_name = crate::schema::users)]
pub struct NewUser<'a> {
    pub id: [u8; 16],
    pub username: &'a str,
    pub password_hash: &'a str,
    pub email: &'a str,
}
