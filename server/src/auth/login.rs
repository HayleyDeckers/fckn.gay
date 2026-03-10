use std::{sync::Arc, time::Instant};

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};
use tracing::Instrument;

use crate::{auth_cache::AuthenticationCache, error::AppError, extract::Form};

#[derive(serde::Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

#[tracing::instrument(skip_all, fields(user = %form.username))]
pub async fn login(
    State(user_database): State<Arc<UserDatabase>>,
    State(auth_cache): State<Arc<AuthenticationCache>>,
    jar: CookieJar,
    Form(form): Form<Login>,
) -> Result<CookieJar, AppError> {
    if let Some(user_id) = user_database
        .validate_and_get_user_id(&form.username, &form.password)
        .instrument(tracing::info_span!("db.validate_credentials"))
        .await
    {
        let token = auth_cache
            .new_token_for(
                form.username,
                user_id,
                Instant::now() + std::time::Duration::from_secs(60 * 60 * 4),
            )
            .instrument(tracing::info_span!("cache.create_token"))
            .await
            .ok_or(AppError::message(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create session 💀",
            ))?;
        tracing::info!("successfully logged in");
        Ok(jar.add(
            Cookie::build(("login-token", token.clone()))
                .domain("127.0.0.1")
                .http_only(true)
                .build(),
        ))
    } else {
        Err(AppError::message(
            StatusCode::UNAUTHORIZED,
            "invalid username or password",
        ))
    }
}

#[tracing::instrument(skip_all)]
pub async fn logout(
    State(auth_cache): State<Arc<AuthenticationCache>>,
    jar: CookieJar,
) -> impl IntoResponse {
    // invalidate the login token if it exists
    if let Some(cookie) = jar.get("login-token") {
        auth_cache.remove_token(cookie.value()).await;
    }
    tracing::info!("logged out");
    // and remove it from the browsers cookie jar
    let jar = jar.remove(Cookie::build("login-token"));
    (jar, Redirect::to("/"))
}
