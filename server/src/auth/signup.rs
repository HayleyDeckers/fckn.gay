use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Redirect};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};
use fckn_gay_validation::{validate_password, validate_username};
use tracing::Instrument;

use crate::{
    captcha::TurnstileVerifier,
    error::AppError,
    extract::{self, ClientIp},
    interfaces::ServerAddress,
};

#[derive(serde::Deserialize)]
pub struct Signup {
    username: String,
    password: String,
    email: String,
    /// Turnstile token — present when captcha is enabled on the frontend
    #[serde(rename = "cf-turnstile-response", default)]
    cf_turnstile_response: Option<String>,
}

#[tracing::instrument(skip_all, fields(user = %form.username))]
pub async fn sign_up(
    State(turnstile): State<Option<Arc<TurnstileVerifier>>>,
    State(user_database): State<Arc<UserDatabase>>,
    State(email): State<Arc<Email>>,
    State(address): State<ServerAddress>,
    ClientIp(client_ip): ClientIp,
    extract::Form(form): extract::Form<Signup>,
) -> Result<StatusCode, AppError> {
    if let Some(verifier) = &turnstile {
        let Some(token) = &form.cf_turnstile_response else {
            return Err(AppError::message(
                StatusCode::FORBIDDEN,
                "captcha token missing — are you a bot? 🤖",
            ));
        };
        let ip = client_ip.as_deref();
        let ok = verifier
            .verify(token, ip)
            .instrument(tracing::info_span!("captcha.verify"))
            .await
            .map_err(|e| {
                AppError::message(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "captcha verification service is having a moment 💀 try again later",
                )
                .with_internal(e)
            })?;
        if !ok {
            return Err(AppError::message(
                StatusCode::FORBIDDEN,
                "captcha verification failed — try again maybe? 🤔",
            ));
        }
    }

    if !user_database
        .is_available(&form.username)
        .instrument(tracing::info_span!("db.check_availability"))
        .await
    {
        return Err(AppError::message(
            StatusCode::CONFLICT,
            "username already taken",
        ));
    }
    let username_result = validate_username(&form.username);
    if !username_result.is_valid() {
        return Err(AppError::message(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "Username validation failed: {}",
                username_result.errors().join(", ")
            ),
        ));
    }

    let password_result = validate_password(&form.password);
    if !password_result.is_valid() {
        return Err(AppError::message(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "Password validation failed: {}",
                password_result.errors().join(", ")
            ),
        ));
    }

    // this is safe for now since we do check in `add_user` but we can race.
    let uuid = user_database
        .add_user(&form.username, &form.password, &form.email)
        .instrument(tracing::info_span!("db.add_user"))
        .await
        .map_err(|e| {
            AppError::message(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create user 💀",
            )
            .with_internal(e)
        })?;
    email
        .send_email(
            "im@fckn.gay",
            &form.email,
            "Sign up for your new account",
            &format!(
                "Hello {},\n\
                Thank you for signing up for an account at fckn.gay.\n\
                Please click the following link to activate your account:\n\
                http://{address}/confirm-signup?token={uuid:?}\n\
                \n\
                if you did not sign up for an account, please ignore this email.",
                &form.username
            ),
        )
        .instrument(tracing::info_span!("email.send_confirmation"))
        .await?;
    tracing::info!("signup successful, confirmation email sent");
    // todo(hayley): if email failed to send, delete the user from the database?
    Ok(StatusCode::CREATED)
}

#[derive(serde::Deserialize)]
pub struct ConfirmSignup {
    token: fckn_gay_user_database::Uuid,
}

#[tracing::instrument(skip_all)]
pub async fn confirm_sign_up(
    State(user_database): State<Arc<UserDatabase>>,
    extract::Query(query): extract::Query<ConfirmSignup>,
) -> Result<Redirect, AppError> {
    user_database
        .activate_user(query.token)
        .instrument(tracing::info_span!("db.activate_user"))
        .await?;
    tracing::info!("user confirmed signup");
    Ok(Redirect::to("/"))
}
