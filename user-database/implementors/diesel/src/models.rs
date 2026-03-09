use diesel::prelude::*;
use fckn_gay_user_database_interface::{
    DnsRecord, DnsRecordId, DnsRecordType, PasswordHash, UserState, Uuid,
};

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
            fields: fckn_gay_user_database_interface::UserFields {
                username,
                email,
                password_hash,
                state,
                created_at,
                last_login,
            },
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

// DNS Record Models
#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = crate::schema::dns_records)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct RawDnsRecord {
    pub id: Vec<u8>,
    pub user_id: Vec<u8>,
    pub provider_key: String,
    pub name: String,
    pub record_type: i32,
    pub content: String,
    pub ttl_seconds: i32,
    pub priority: Option<i32>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

impl From<RawDnsRecord> for DnsRecord {
    fn from(raw: RawDnsRecord) -> Self {
        let record_type =
            DnsRecordType::try_from(raw.record_type).expect("invalid DNS record type in database");

        Self {
            name: raw.name,
            record_type,
            content: raw.content,
            ttl_seconds: raw.ttl_seconds as u32,
            priority: raw.priority.map(|p| p as u16),
        }
    }
}

impl From<RawDnsRecord> for DnsRecordId {
    fn from(raw: RawDnsRecord) -> Self {
        let id = Uuid::from_slice_le(&raw.id).unwrap();
        DnsRecordId(id)
    }
}

#[derive(Insertable, Debug)]
#[diesel(table_name = crate::schema::dns_records)]
pub struct NewDnsRecord<'a> {
    pub id: [u8; 16],
    pub user_id: [u8; 16],
    pub provider_key: &'a str,
    pub name: &'a str,
    pub record_type: i32,
    pub content: &'a str,
    pub ttl_seconds: i32,
    pub priority: Option<i32>,
}

impl<'a> NewDnsRecord<'a> {
    pub fn from_interface(
        record_id: DnsRecordId,
        user_id: Uuid,
        record: &'a DnsRecord,
        provider_key: &'a str,
    ) -> Self {
        Self {
            id: record_id.0.to_bytes_le(),
            user_id: user_id.to_bytes_le(),
            provider_key,
            name: &record.name,
            record_type: i32::from(record.record_type),
            content: &record.content,
            ttl_seconds: record.ttl_seconds as i32,
            priority: record.priority.map(|p| p as i32),
        }
    }
}
