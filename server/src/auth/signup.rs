use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};
use fckn_gay_validation::{is_valid_password, is_valid_username};
use tokio::sync::Mutex;

use crate::error::AppError;

#[derive(serde::Deserialize)]
pub struct Signup {
    username: String,
    password: String,
    email: String,
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
    let username_result = is_valid_username(&form.username);
    if !username_result.is_valid {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            anyhow!(
                "Username validation failed: {}",
                username_result.errors.join(", ")
            ),
        ));
    }

    let password_result = is_valid_password(&form.password);
    if !password_result.is_valid {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            anyhow!(
                "Password validation failed: {}",
                password_result.errors.join(", ")
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
    use fckn_gay_validation::{is_valid_password, is_valid_username};
    #[test]
    fn valid_password() {
        assert!(is_valid_password("aB1.aB1.aB1.").is_valid);
    }

    #[test]
    fn password_min_max_length() {
        assert!(!is_valid_password("aB1.").is_valid);
        assert!(is_valid_password(
            "aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1."
        ).is_valid);
        assert!(!is_valid_password(
            "aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.aB1.."
        ).is_valid);
    }
    #[test]
    fn password_character_set() {
        // no punc
        assert!(!is_valid_password("aB1xaB1xaB1xa").is_valid);
        // no lowercase
        assert!(!is_valid_password("XB1.XB1.XB1.").is_valid);
        // no uppercase
        assert!(!is_valid_password("ab1.ab1.ab1.").is_valid);
        // no numbers
        assert!(!is_valid_password("aBi.aBi.aBi.").is_valid);
    }

    #[test]
    fn valid_username() {
        assert!(is_valid_username("username").is_valid);
        assert!(is_valid_username("i").is_valid);
    }

    #[test]
    fn username_min_max_length() {
        assert!(!is_valid_username("").is_valid);
        assert!(is_valid_username("x").is_valid);
        let len_63 = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        assert_eq!(len_63.len(), 63);
        let len_64 = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        assert_eq!(len_64.len(), 64);
        assert!(!is_valid_username(len_64).is_valid);
    }

    #[test]
    fn username_character_set() {
        // not start with -
        assert!(!is_valid_username("-username").is_valid);
        // not end with -
        assert!(!is_valid_username("username-").is_valid);
        // all lowercase
        assert!(!is_valid_username("uSeRnaMe").is_valid);
        // do allow all digits
        assert!(is_valid_username("5").is_valid);
        // don't allow emoji
        assert!(!is_valid_username("🐛").is_valid);
        // but punycode should work
        assert!(is_valid_username("xn--jo8h").is_valid);
        // no underscores
        assert!(!is_valid_username("user_name").is_valid);
        // also not at the start
        assert!(!is_valid_username("_username").is_valid);
        // no whitespace
        assert!(!is_valid_username("user name").is_valid);
        // no dots
        assert!(!is_valid_username("user.name").is_valid);
        // no ü
        assert!(!is_valid_username("üsername").is_valid);
    }
}
