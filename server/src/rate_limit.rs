//! Rate limiting middleware for auth and API routes.
//!
//! Uses tower_governor under the hood with two strategies:
//! - IP-based rate limiting for auth routes (login, signup, etc) via SmartIpKeyExtractor
//! - User-based rate limiting for API routes via custom UserKeyExtractor
//!
//! Rate limit info is exposed via X-RateLimit-* headers so clients can auto-backoff.

use axum::{body::Body, http::Request};
use tower_governor::{
    GovernorLayer,
    errors::GovernorError,
    governor::GovernorConfigBuilder,
    key_extractor::{KeyExtractor, SmartIpKeyExtractor},
};

use crate::{auth_cache::AuthenticatedFor, interfaces::RateLimitConfig};

/// Key extractor that uses the authenticated user's ID for rate limiting.
/// This should only be used on routes that have already passed through auth middleware,
/// so the AuthenticatedFor extension should always be present.
#[derive(Debug, Clone)]
pub struct UserKeyExtractor;

impl KeyExtractor for UserKeyExtractor {
    type Key = String;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        req.extensions()
            .get::<AuthenticatedFor>()
            .map(|auth: &AuthenticatedFor| auth.user_id().to_string())
            .ok_or_else(|| GovernorError::UnableToExtractKey)
    }
}

/// Creates a GovernorLayer for auth routes (login, signup, etc).
/// Uses SmartIpKeyExtractor which checks x-forwarded-for, x-real-ip, forwarded headers
/// before falling back to peer IP - works nicely behind reverse proxies.
pub fn auth_rate_limit_layer(
    config: &RateLimitConfig,
) -> GovernorLayer<SmartIpKeyExtractor, governor::middleware::StateInformationMiddleware, Body> {
    let governor_conf = GovernorConfigBuilder::default()
        .per_second(config.auth_per_seconds)
        .burst_size(config.auth_burst_size)
        .key_extractor(SmartIpKeyExtractor)
        .use_headers() // exposes x-ratelimit-* headers to clients
        .finish()
        .expect("auth rate limit config should be valid");

    GovernorLayer::new(governor_conf)
}

/// Creates a GovernorLayer for API routes.
/// Uses UserKeyExtractor to rate limit per authenticated user.
pub fn api_rate_limit_layer(
    config: &RateLimitConfig,
) -> GovernorLayer<UserKeyExtractor, governor::middleware::StateInformationMiddleware, Body> {
    let governor_conf = GovernorConfigBuilder::default()
        .per_second(config.api_per_seconds)
        .burst_size(config.api_burst_size)
        .key_extractor(UserKeyExtractor)
        .use_headers() // exposes x-ratelimit-* headers to clients
        .finish()
        .expect("api rate limit config should be valid");

    GovernorLayer::new(governor_conf)
}
