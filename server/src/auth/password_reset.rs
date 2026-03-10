use std::{sync::Arc, time::Instant};

use axum::{extract::State, http::StatusCode};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{
    Database as UserDatabase, Interface as UserDatabaseInterface, PasswordHash,
};
use fckn_gay_validation::validate_password;
use serde::{Deserialize, Deserializer};
use tracing::Instrument;

use crate::{
    auth_cache::{AuthenticationCache, PasswordResetCache},
    error::AppError,
    extract::{Form, Json},
    interfaces::ServerAddress,
};

#[derive(serde::Deserialize)]
pub struct PasswordReset {
    username_or_email: String,
}

#[tracing::instrument(skip_all, fields(target = %form.username_or_email))]
pub async fn request_password_reset(
    State(email): State<Arc<Email>>,
    State(password_reset_cache): State<PasswordResetCache>,
    State(user_database): State<Arc<UserDatabase>>,
    State(address): State<ServerAddress>,
    Form(form): Form<PasswordReset>,
) -> Result<StatusCode, AppError> {
    tracing::info!("password reset requested");
    if let Some(user) = user_database
        .get_user_by_username_or_email(&form.username_or_email)
        .instrument(tracing::info_span!("db.lookup_user"))
        .await?
    {
        if !user.is_active() {
            tracing::warn!(
                user = %user.username,
                state = ?user.state,
                "password reset requested for non-active user"
            );
            return Ok(StatusCode::OK);
        }
        let password_reset_token = password_reset_cache
            .new_token_for(
                user.username.clone(),
                user.id,
                Instant::now() + std::time::Duration::from_secs(15 * 60),
            )
            .instrument(tracing::info_span!("cache.create_reset_token"))
            .await
            .ok_or(AppError::message(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to generate password reset token 💀",
            ))?;
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
                    http://{address}/reset-password/?token={password_reset_token}\n",
                    &user.username
                ),
            )
            .instrument(tracing::info_span!("email.send_reset"))
            .await?;
    }
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

#[tracing::instrument(skip_all)]
pub async fn reset_password(
    State(password_reset_cache): State<PasswordResetCache>,
    State(user_database): State<Arc<UserDatabase>>,
    State(auth_cache): State<Arc<AuthenticationCache>>,
    Form(form): Form<HandlePasswordReset>,
) -> Result<Json<String>, AppError> {
    // Atomically remove the token so concurrent requests can't both succeed
    let (username, user_id) = password_reset_cache
        .take_valid_token(&form.token)
        .await
        .ok_or(AppError::message(
            StatusCode::UNAUTHORIZED,
            "invalid or expired password reset token",
        ))?;
    user_database
        .update_user_password(user_id, PasswordHash::new(&form.new_password.0))
        .instrument(tracing::info_span!("db.update_password"))
        .await?;
    auth_cache
        .invalidate_all_tokens_for_user(user_id)
        .instrument(tracing::info_span!("cache.invalidate_tokens"))
        .await;
    tracing::info!(user = %username, "password reset completed");
    Ok(Json(username))
}
