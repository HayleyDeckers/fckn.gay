pub mod dns;

use axum::{Router, middleware::from_fn_with_state};

use crate::{Interfaces, auth_cache::add_authorization_or_unauthorized, rate_limit};

/// Main API router that combines all API endpoints and applies auth + rate limiting.
pub fn router(appstate: Interfaces) -> Router<Interfaces> {
    let rate_limiter = rate_limit::api_rate_limit_layer(&appstate.rate_limit);
    Router::new()
        .merge(dns::router(appstate.clone()))
        .layer(rate_limiter)
        .layer(from_fn_with_state(
            appstate.auth_cache.clone(),
            add_authorization_or_unauthorized,
        ))
}
