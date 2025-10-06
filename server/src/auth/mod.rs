pub mod login;
pub mod signup;

use axum::Router;

use crate::Interfaces;

/// Auth router that combines login and signup endpoints
pub fn router(appstate: Interfaces) -> Router<Interfaces> {
    Router::new()
        .route("/login", axum::routing::post(login::login))
        .route("/logout", axum::routing::get(login::logout))
        .route("/signup", axum::routing::post(signup::sign_up))
        .route(
            "/confirm-signup/{uuid}",
            axum::routing::get(signup::confirm_sign_up),
        )
        .with_state(appstate)
}
