pub mod dns;

use axum::Router;

use crate::Interfaces;

/// Main API router that combines all API endpoints
pub fn router(appstate: Interfaces) -> Router<Interfaces> {
    Router::new().merge(dns::router(appstate.clone()))
}
