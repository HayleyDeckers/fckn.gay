use std::{collections::BTreeMap, sync::Arc, time::Instant};

use axum::{
    extract::{FromRequestParts, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use fckn_gay_user_database::Uuid;
use tokio::sync::RwLock;

pub struct LoginToken {
    expires_at: Instant,
    username: String,
    user_id: Uuid,
}

impl LoginToken {
    fn new(expires_at: Instant, username: String, user_id: Uuid) -> Self {
        Self {
            expires_at,
            username,
            user_id,
        }
    }
    fn is_valid(&self) -> bool {
        self.expires_at > Instant::now()
    }
    pub fn username(&self) -> &str {
        &self.username
    }
    pub fn user_id(&self) -> Uuid {
        self.user_id
    }
}

pub struct AuthenticationCache {
    // login-token -> user-id
    db: RwLock<BTreeMap<String, LoginToken>>,
}

/// Newtype wrapper so axum's `FromRef` can distinguish the password-reset cache
/// from the login-session cache — they're both `AuthenticationCache` under the hood
/// but we need separate `FromRef` impls so `State(…)` grabs the right one.
#[derive(Clone)]
pub struct PasswordResetCache(pub Arc<AuthenticationCache>);

impl std::ops::Deref for PasswordResetCache {
    type Target = AuthenticationCache;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AuthenticationCache {
    pub fn new() -> Self {
        Self {
            db: RwLock::new(BTreeMap::new()),
        }
    }

    pub async fn add_token(
        &self,
        token: String,
        username: String,
        user_id: Uuid,
        expires_at: Instant,
    ) {
        self.db
            .write()
            .await
            .insert(token, LoginToken::new(expires_at, username, user_id));
    }

    pub async fn remove_token(&self, token: &str) -> Option<LoginToken> {
        let mut wlock = self.db.write().await;
        wlock.remove(token)
    }

    pub async fn new_token_for(
        &self,
        username: String,
        user_id: Uuid,
        expires_at: Instant,
    ) -> Option<String> {
        let (Ok(hi), Ok(lo)) = (getrandom::u64(), getrandom::u64()) else {
            return None;
        };
        let token = format!("{hi:016x}{lo:016x}");
        self.add_token(token.clone(), username, user_id, expires_at)
            .await;
        Some(token)
    }

    /// Atomically removes a token and returns (username, user_id) if it was valid.
    /// Prevents race conditions where two concurrent requests could both validate
    /// the same token.
    pub async fn take_valid_token(&self, token: &str) -> Option<(String, Uuid)> {
        let removed = self.db.write().await.remove(token)?;
        if removed.is_valid() {
            Some((removed.username().to_string(), removed.user_id()))
        } else {
            None
        }
    }

    pub async fn get_user_from_token(&self, token: &str) -> Option<(String, Uuid)> {
        let should_remove = {
            let rlock = self.db.read().await;
            let value = rlock.get(token);
            if value.is_some_and(LoginToken::is_valid) {
                return value.map(|v| (v.username().to_string(), v.user_id()));
            }
            value.is_some()
        };
        if should_remove {
            self.db.write().await.remove(token);
        }
        None
    }

    pub async fn invalidate_all_tokens_for_user(&self, user_id: Uuid) {
        let mut wlock = self.db.write().await;
        wlock.retain(|_, token| token.user_id() != user_id);
    }
}

#[derive(Clone, Debug)]
pub struct AuthenticatedFor {
    user_id: Uuid,
    username: String,
}

impl AuthenticatedFor {
    pub fn new(user_id: Uuid, username: String) -> Self {
        Self { user_id, username }
    }

    pub fn user_id(&self) -> Uuid {
        self.user_id
    }

    pub fn username(&self) -> &str {
        &self.username
    }
}

/// middleware function that checks if the user is authorized
/// through the use of a `login-token` cookie.
/// If the user is authorized, it adds the user id to the request's extensions.
/// if the user is not authorized, it does nothing.
pub async fn add_authorization(state: &AuthenticationCache, request: &mut Request) -> bool {
    let jar = CookieJar::from_headers(request.headers());
    if let Some(cookie) = jar.get("login-token") {
        // we don't care about the cookie domain, path, etc.
        // those are for the browser to care about
        let token = cookie.value();
        if let Some((username, user_id)) = state.get_user_from_token(token).await {
            tracing::debug!(user = %username, user_id = %user_id, "authorized via token");
            request
                .extensions_mut()
                .insert(AuthenticatedFor::new(user_id, username));
            return true;
        }
        tracing::trace!("login token present but invalid or expired");
    } else {
        tracing::trace!("no login-token cookie");
    }
    false
}

impl<S: Send + Sync + 'static> FromRequestParts<S> for AuthenticatedFor {
    type Rejection = crate::error::AppError;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthenticatedFor>()
            .cloned()
            .ok_or(crate::error::AppError::message(
                StatusCode::UNAUTHORIZED,
                "not authenticated",
            ))
    }
}

pub async fn add_authorization_or_unauthorized(
    State(state): State<Arc<AuthenticationCache>>,
    mut request: Request,
    next: Next,
) -> Result<Response, crate::error::AppError> {
    if add_authorization(&state, &mut request).await {
        Ok(next.run(request).await)
    } else {
        Err(crate::error::AppError::message(
            StatusCode::UNAUTHORIZED,
            "not authenticated",
        ))
    }
}

pub async fn redirect_if_unauthorized(
    State(state): State<Arc<AuthenticationCache>>,
    mut request: Request,
    next: Next,
) -> Result<Response, Redirect> {
    if add_authorization(&state, &mut request).await {
        Ok(next.run(request).await)
    } else {
        Err(Redirect::to("/"))
    }
}
