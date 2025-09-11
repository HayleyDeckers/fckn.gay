// @generated automatically by Diesel CLI.

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
