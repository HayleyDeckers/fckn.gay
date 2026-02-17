pub mod login;
pub mod signup;

use axum::Router;

use crate::{Interfaces, rate_limit};

/// Auth router that combines login/signup endpoints and applies IP-based rate limiting.
/// Prevents brute force on login/signup endpoints.
pub fn router(appstate: Interfaces) -> Router<Interfaces> {
    let rate_limiter = rate_limit::auth_rate_limit_layer(&appstate.rate_limit);
    Router::new()
        .route("/login", axum::routing::post(login::login))
        .route("/logout", axum::routing::get(login::logout))
        .route("/signup", axum::routing::post(signup::sign_up))
        .route(
            "/confirm-signup/{uuid}",
            axum::routing::get(signup::confirm_sign_up),
        )
        .with_state(appstate)
        .layer(rate_limiter)
}
