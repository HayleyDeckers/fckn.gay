use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};
use fckn_gay_validation::{validate_password, validate_username};
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
    let username_result = validate_username(&form.username);
    if !username_result.is_valid() {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            anyhow!(
                "Username validation failed: {}",
                username_result.errors().join(", ")
            ),
        ));
    }

    let password_result = validate_password(&form.password);
    if !password_result.is_valid() {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            anyhow!(
                "Password validation failed: {}",
                password_result.errors().join(", ")
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
