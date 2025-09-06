use std::time::Instant;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;

use axum::{
    extract::{FromRequestParts, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;

pub struct LoginToken {
    expires_at: Instant,
    user_id: String,
}

impl LoginToken {
    fn new(expires_at: Instant, user_id: String) -> Self {
        Self {
            expires_at,
            user_id,
        }
    }
    fn is_valid(&self) -> bool {
        self.expires_at > Instant::now()
    }
    fn user_id(&self) -> &str {
        &self.user_id
    }
}

pub struct AuthenticationCache {
    // login-token -> user-id
    db: RwLock<BTreeMap<String, LoginToken>>,
}

impl AuthenticationCache {
    pub fn new() -> Self {
        Self {
            db: RwLock::new(BTreeMap::new()),
        }
    }

    pub async fn add_token(&self, token: String, user_id: String, expires_at: Instant) {
        self.db
            .write()
            .await
            .insert(token, LoginToken::new(expires_at, user_id));
    }

    pub async fn remove_token(&self, token: &str) -> Option<LoginToken> {
        let mut wlock = self.db.write().await;
        wlock.remove(token)
    }

    pub async fn new_token_for(&self, user_id: String, expires_at: Instant) -> Option<String> {
        let (Ok(hi), Ok(lo)) = (getrandom::u64(), getrandom::u64()) else {
            return None;
        };
        let token = format!("{:016x}{:016x}", hi, lo);
        self.add_token(token.clone(), user_id, expires_at).await;
        Some(token)
    }

    pub async fn get_user_id_from_token(&self, token: &str) -> Option<String> {
        let should_remove = {
            let rlock = self.db.read().await;
            let value = rlock.get(token);
            if value.is_some_and(LoginToken::is_valid) {
                return value.map(|v| v.user_id().to_string());
            }
            value.is_some()
        };
        if should_remove {
            self.db.write().await.remove(token);
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct AuthenticatedFor(String);
impl AuthenticatedFor {
    pub fn new(user_id: String) -> Self {
        Self(user_id)
    }

    pub fn user_id(&self) -> &str {
        &self.0
    }
}

/// middleware function that checks if the user is authorized
/// through the use of a `login-token` cookie.
/// If the user is authorized, it adds the user id to the request's extensions.
/// if the user is not authorized, it does nothing.
pub async fn add_authorization(state: &AuthenticationCache, request: &mut Request) -> bool {
    let jar = CookieJar::from_headers(&request.headers());
    if let Some(cookie) = jar.get("login-token") {
        // we don't care about the cookie domain, path, etc.
        // those are for the browser to care about
        let token = cookie.value();
        if let Some(authorized_for) = state.get_user_id_from_token(token).await {
            println!("User {} authorized with token {}", authorized_for, token);
            request
                .extensions_mut()
                .insert(AuthenticatedFor::new(authorized_for));
            return true;
        }
    }
    false
}

impl<S: Send + Sync + 'static> FromRequestParts<S> for AuthenticatedFor {
    type Rejection = StatusCode;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthenticatedFor>()
            //could we get rid of this clone?
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

pub async fn add_authorization_or_redirect(
    State(state): State<Arc<AuthenticationCache>>,
    mut request: Request,
    next: Next,
) -> Result<Response, Redirect> {
    if add_authorization(&state, &mut request).await {
        Ok(next.run(request).await)
    } else {
        let redirect = Redirect::to("/");
        Err(redirect)
    }
}
