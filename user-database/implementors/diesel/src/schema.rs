// @generated automatically by Diesel CLI.

diesel::table! {
    dns_records (id) {
        id -> Binary,
        user_id -> Binary,
        provider_key -> Text,
        name -> Text,
        record_type -> Integer,
        content -> Text,
        ttl_seconds -> Integer,
        priority -> Nullable<Integer>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    users (id) {
        id -> Binary,
        username -> Text,
        password_hash -> Text,
        email -> Text,
        state -> Integer,
        created_at -> Timestamp,
        last_login -> Nullable<Timestamp>,
    }
}

diesel::joinable!(dns_records -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(dns_records, users,);
