use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};
use tokio::sync::Mutex;

use crate::error::AppError;

#[derive(serde::Deserialize)]
pub struct Signup {
    username: String,
    password: String,
    email: String,
}

pub fn is_valid_username(username: &str) -> bool {
    // must be a valid dns label, which is
    // atleast 1 character, max 63
    // ascii alphanumeric or '-'
    // may not start or end with a '-'.
    // we also require it is all lowercase, since dns is case-insensitive
    let len = username.len();
    if len == 0 || len > 63 {
        return false;
    }
    if username.starts_with('-') || username.ends_with('-') {
        return false;
    }
    for char in username.chars() {
        if !(char.is_ascii_lowercase() || char.is_ascii_digit() || (char == '-')) {
            return false;
        }
    }
    true
}

pub fn is_valid_password(password: &str) -> bool {
    // must be between 12 and 128 characters
    // must contain at least one uppercase letter, one lowercase letter, one digit, and one punctuation character
    if password.len() < 12
        || password.len() > 128
        || !password.chars().any(|c| c.is_ascii_uppercase())
        || !password.chars().any(|c| c.is_ascii_lowercase())
        || !password.chars().any(|c| c.is_ascii_digit())
        || !password.chars().any(|c| c.is_ascii_punctuation())
    {
        return false;
    }
    true
}

pub async fn sign_up(
    //todo(hayley): remove these locks
    State(user_database): State<Arc<Mutex<UserDatabase>>>,
    State(email): State<Arc<Mutex<Email>>>,
    Form(form): Form<Signup>,
) -> Result<StatusCode, AppError> {
    let db = user_database.lock().await;
    if !db.is_available(&form.username).await {
        return Ok(StatusCode::CONFLICT);
    }
    if !is_valid_username(&form.username) {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            anyhow!(
                "Username must be between 1 and 63 characters, lowercase ascii alphanumeric or '-', and must not start or end with '-'"
            ),
        ));
    }
    if !is_valid_password(&form.password) {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            anyhow::anyhow!(
                "Password must be between 12 and 128 characters and contain at least one uppercase letter, one lowercase letter, one digit, and one punctuation character"
            ),
        ));
    }

    // this is safe for now since we do check in `add_user` but we can race.
    let Ok(uuid) = db
        .add_user(&form.username, &form.password, &form.email)
        .await
    else {
        return Ok(StatusCode::INTERNAL_SERVER_ERROR);
    };
    email
        .lock()
        .await
        .send_email(
            "im@fckn.gay",
            &form.email,
            "Sign up for your new account",
            &format!(
                "Hello {},\n\
                Thank you for signing up for an account at fckn.gay.\n\
                Please click the following link to activate your account:\n\
                http://127.0.0.1:8080/confirm-signup/{uuid:?}\n\
                \n\
                if you did not sign up for an account, please ignore this email.",
                &form.username
            ),
        )
        .await?;
    // todo(hayley): if email failed to send, delete the user from the database?
    Ok(StatusCode::CREATED)
}

pub async fn confirm_sign_up(
    //todo(hayley): remove these locks
    State(user_database): State<Arc<Mutex<UserDatabase>>>,
    Path(uuid): Path<fckn_gay_user_database::Uuid>,
) -> Result<String, AppError> {
    let db = user_database.lock().await;
    db.activate_user(uuid).await?;
    Ok("Account activated, you can now <a href=\"/login\">login</a>".into())
}

#[cfg(test)]
mod tests {
    use super::is_valid_password;
    use crate::auth::signup::is_valid_username;
    #[test]
    fn valid_password() {
        assert!(is_valid_password("aB1.aB1.aB1."));
    }

    #[test]
    fn password_min_max_length() {
        assert!(!is_valid_password("aB1."));
        assert!(is_valid_password(
            "aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1."
        ));
        assert!(!is_valid_password(
            "aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.."
        ));
    }
    #[test]
    fn password_character_set() {
        // no punc
        assert!(!is_valid_password("aB1xaB1xaB1xa"));
        // no lowercase
        assert!(!is_valid_password("XB1.XB1.XB1."));
        // no uppercase
        assert!(!is_valid_password("ab1.ab1.ab1."));
        // no numbers
        assert!(!is_valid_password("aBi.aBi.aBi."));
    }

    #[test]
    fn valid_username() {
        assert!(is_valid_username("username"));
        assert!(is_valid_username("i"));
    }

    #[test]
    fn username_min_max_length() {
        assert!(!is_valid_username(""));
        assert!(is_valid_username("x"));
        let len_63 = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        assert_eq!(len_63.len(), 63);
        let len_64 = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        assert_eq!(len_64.len(), 64);
        assert!(!is_valid_username(len_64));
    }

    #[test]
    fn username_character_set() {
        // not start with -
        assert!(!is_valid_username("-username"));
        // not end with -
        assert!(!is_valid_username("username-"));
        // all lowercase
        assert!(!is_valid_username("uSeRnaMe"));
        // do allow all digits
        assert!(is_valid_username("5"));
        // don't allow emoji
        assert!(!is_valid_username("🐛"));
        // but punycode should work
        assert!(is_valid_username("xn--jo8h"));
        // no underscores
        assert!(!is_valid_username("user_name"));
        // also not at the start
        assert!(!is_valid_username("_username"));
        // no whitespace
        assert!(!is_valid_username("user name"));
        // no dots
        assert!(!is_valid_username("user.name"));
        // no ü
        assert!(!is_valid_username("üsername"));
    }
}
