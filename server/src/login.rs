use crate::auth_cache::AuthenticationCache;
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};
use std::{sync::Arc, time::Instant};
use tokio::sync::Mutex;

#[derive(serde::Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

pub async fn login(
    State(user_database): State<Arc<Mutex<UserDatabase>>>,
    State(auth_cache): State<Arc<AuthenticationCache>>,
    jar: CookieJar,
    Form(form): Form<Login>,
) -> Result<CookieJar, StatusCode> {
    if user_database
        .lock()
        .await
        .is_valid(&form.username, &form.password)
        .await
    {
        let token = auth_cache
            .new_token_for(
                form.username,
                Instant::now() + std::time::Duration::from_hours(4),
            )
            .await
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(jar.add(
            Cookie::build(("login-token", token.clone()))
                .domain("127.0.0.1")
                .http_only(true)
                .build(),
        ))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

pub async fn logout(
    State(auth_cache): State<Arc<AuthenticationCache>>,
    jar: CookieJar,
) -> impl IntoResponse {
    // invalidate the login token if it exists
    if let Some(cookie) = jar.get("login-token") {
        auth_cache.remove_token(cookie.value()).await;
    }
    // and remove it from the browsers cookie jar
    let jar = jar.remove(Cookie::build("login-token"));
    (jar, Redirect::to("/"))
}

#[derive(serde::Deserialize)]
pub struct Signup {
    username: String,
    password: String,
    email: String,
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
) -> Result<StatusCode, crate::error::AppError> {
    let db = user_database.lock().await;
    if !db.is_available(&form.username).await {
        return Ok(StatusCode::CONFLICT);
    }
    if !is_valid_password(&form.password) {
        return Err(crate::error::AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            anyhow::anyhow!(
                "Password must be between 12 and 128 characters and contain at least one uppercase letter, one lowercase letter, one digit, and one punctuation character"
            ),
        ));
    }

    //todo(hayley): hash the password before storing it, and validate it
    //todo(hayley): check here for availablity on the error message and return conflict if not available.
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
                http://127.0.0.1:8080/confirm-sign-up/{uuid:?}\n\
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
) -> Result<String, crate::error::AppError> {
    let db = user_database.lock().await;
    db.activate_user(uuid).await?;
    Ok("Account activated, you can now <a href=\"/login\">login</a>".into())
}

#[cfg(test)]
mod tests {
    use super::is_valid_password;
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
}
