use std::{sync::Arc, time::Instant};

use anyhow::anyhow;
use axum::{
    extract::{Form, State},
    http::StatusCode,
};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{
    Database as UserDatabase, Interface as UserDatabaseInterface, PasswordHash,
};
use fckn_gay_validation::validate_password;
use serde::{Deserialize, Deserializer};

use crate::{
    auth_cache::{AuthenticationCache, PasswordResetCache},
    error::AppError,
};

#[derive(serde::Deserialize)]
pub struct PasswordReset {
    username_or_email: String,
}

pub async fn request_password_reset(
    State(email): State<Arc<Email>>,
    State(password_reset_cache): State<PasswordResetCache>,
    State(user_database): State<Arc<UserDatabase>>,
    Form(form): Form<PasswordReset>,
) -> Result<StatusCode, AppError> {
    // grab user from user_database, by username or email
    if let Some(user) = user_database
        .get_user_by_username_or_email(&form.username_or_email)
        .await?
    {
        // if user found,
        //  - generate the password reset token and add it to the password reset cache
        let password_reset_token = password_reset_cache
            .new_token_for(
                user.username.clone(),
                user.id,
                Instant::now() + std::time::Duration::from_secs(15 * 60),
            )
            .await
            .ok_or(AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                anyhow!("Failed to generate password reset token"),
            ))?;
        //  - send email with reset link
        email
            .send_email(
                "im@fckn.gay",
                &user.email,
                "Reset your password",
                &format!(
                    "Hello {},\n\
                Someone (hopefully you) has requested a password reset for your account at fckn.gay.\n\
                Please click the following link to reset your password:\n\
                (if you did not request a password reset, please ignore this email)\n\
                http://127.0.0.1:8080/reset-password/?token={password_reset_token}\n",
                    &user.username
                ),
            )
            .await?;
    }
    // return an ok response, even if user is not found
    Ok(StatusCode::OK)
}

#[derive(serde::Deserialize)]
pub struct HandlePasswordReset {
    new_password: ValidPassword,
    token: String,
}

struct ValidPassword(String);

impl<'de> Deserialize<'de> for ValidPassword {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if validate_password(&s).is_valid() {
            Ok(ValidPassword(s))
        } else {
            Err(serde::de::Error::custom("Invalid password"))
        }
    }
}

pub async fn reset_password(
    State(password_reset_cache): State<PasswordResetCache>,
    State(user_database): State<Arc<UserDatabase>>,
    State(auth_cache): State<Arc<AuthenticationCache>>,
    Form(form): Form<HandlePasswordReset>,
) -> Result<StatusCode, AppError> {
    // check that the token is valid (present in password reset cache and not expired)
    let (_username, user_id) = password_reset_cache
        .get_user_from_token(&form.token)
        .await
        .ok_or(AppError::new(
            StatusCode::UNAUTHORIZED,
            anyhow!("Invalid or expired password reset token"),
        ))?;
    // invalidate the token from the password reset cache
    password_reset_cache
        .remove_token(&form.token)
        .await
        .ok_or(AppError::new(
            StatusCode::UNAUTHORIZED,
            anyhow!("Invalid or expired password reset token"),
        ))?;
    // update the user's password in the database
    user_database
        .update_user_password(user_id, PasswordHash::new(&form.new_password.0))
        .await?;
    // invalidate existing logins
    auth_cache.invalidate_all_tokens_for_user(user_id).await;
    Ok(StatusCode::OK)
}
