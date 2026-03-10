use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Redirect};
use fckn_gay_email::{Email, Interface as EmailInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};
use fckn_gay_validation::{validate_password, validate_username};
use tower_governor::key_extractor::{KeyExtractor, SmartIpKeyExtractor};

use crate::{captcha::TurnstileVerifier, error::AppError, extract, interfaces::ServerAddress};

#[derive(serde::Deserialize)]
pub struct Signup {
    username: String,
    password: String,
    email: String,
    /// Turnstile token — present when captcha is enabled on the frontend
    #[serde(rename = "cf-turnstile-response", default)]
    cf_turnstile_response: Option<String>,
}

pub async fn sign_up(
    State(turnstile): State<Option<Arc<TurnstileVerifier>>>,
    State(user_database): State<Arc<UserDatabase>>,
    State(email): State<Arc<Email>>,
    State(address): State<ServerAddress>,
    request: axum::extract::Request,
) -> Result<StatusCode, AppError> {
    // Grab the client IP before consuming the request for form extraction.
    // Uses the same SmartIpKeyExtractor as the rate limiter (x-forwarded-for → x-real-ip → peer).
    let client_ip = SmartIpKeyExtractor
        .extract(&request)
        .ok()
        .map(|k| k.to_string());
    let extract::Form(form): extract::Form<Signup> =
        <extract::Form<Signup> as axum::extract::FromRequest<_>>::from_request(request, &())
            .await?;

    if let Some(verifier) = &turnstile {
        let Some(token) = &form.cf_turnstile_response else {
            return Err(AppError::message(
                StatusCode::FORBIDDEN,
                "captcha token missing — are you a bot? 🤖",
            ));
        };
        let ip = client_ip.as_deref();
        let ok = verifier.verify(token, ip).await.map_err(|e| {
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

    if !user_database.is_available(&form.username).await {
        return Ok(StatusCode::CONFLICT);
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
    let Ok(uuid) = user_database
        .add_user(&form.username, &form.password, &form.email)
        .await
    else {
        return Ok(StatusCode::INTERNAL_SERVER_ERROR);
    };
    email
        .send_email(
            "im@fckn.gay",
            &form.email,
            "Sign up for your new account",
            &format!(
                "Hello {},\n\
                Thank you for signing up for an account at fckn.gay.\n\
                Please click the following link to activate your account:\n\
                http://{address}/confirm-signup/{uuid:?}\n\
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
    State(user_database): State<Arc<UserDatabase>>,
    axum::extract::Path(uuid): axum::extract::Path<fckn_gay_user_database::Uuid>,
) -> Result<Redirect, AppError> {
    user_database.activate_user(uuid).await?;
    Ok(Redirect::to("/"))
}
