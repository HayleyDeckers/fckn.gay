use std::{sync::Arc, time::Instant};

use axum::{
    extract::{Form, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};

use crate::auth_cache::AuthenticationCache;

#[derive(serde::Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

pub async fn login(
    State(user_database): State<Arc<UserDatabase>>,
    State(auth_cache): State<Arc<AuthenticationCache>>,
    jar: CookieJar,
    Form(form): Form<Login>,
) -> Result<CookieJar, StatusCode> {
    if let Some(user_id) = user_database
        .validate_and_get_user_id(&form.username, &form.password)
        .await
    {
        let token = auth_cache
            .new_token_for(
                form.username,
                user_id,
                Instant::now() + std::time::Duration::from_secs(60 * 60 * 4),
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
