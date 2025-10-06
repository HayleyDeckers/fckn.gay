use axum::{Router, middleware::from_fn_with_state};

use crate::auth_cache::redirect_if_unauthorized;

pub fn router(appstate: crate::Interfaces, user_folder: &str) -> Router<crate::Interfaces> {
    let serve_dir = tower_http::services::ServeDir::new(user_folder);
    Router::new()
        .fallback_service(serve_dir)
        .layer(from_fn_with_state(
            appstate.auth_cache.clone(),
            redirect_if_unauthorized,
        ))
        .with_state(appstate)
}
