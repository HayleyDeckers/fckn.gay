//! Cloudflare Turnstile captcha verification.
//!
//! Provides a `TurnstileVerifier` that holds a reusable HTTP client and the
//! secret key, plus a public config endpoint so the frontend can conditionally
//! load the widget.

use std::sync::Arc;

use axum::{Json, extract::State};
use fckn_gay_secret::Secret;
use serde::{Deserialize, Serialize};

const SITEVERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

/// Holds everything needed to verify Turnstile tokens at runtime.
/// Built once at startup from `TurnstileConfig`, then shared via `Arc`.
pub struct TurnstileVerifier {
    site_key: String,
    secret_key: Secret,
    client: reqwest::Client,
}

impl TurnstileVerifier {
    pub fn new(site_key: String, secret_key: fckn_gay_secret::Secret) -> Self {
        Self {
            site_key,
            secret_key,
            client: reqwest::Client::new(),
        }
    }

    pub fn site_key(&self) -> &str {
        &self.site_key
    }

    /// Asks Cloudflare whether the token the client submitted is legit.
    pub async fn verify(&self, token: &str, remote_ip: Option<&str>) -> anyhow::Result<bool> {
        let mut form = vec![("secret", self.secret_key.expose()), ("response", token)];
        if let Some(ip) = remote_ip {
            form.push(("remoteip", ip));
        }

        let resp = self
            .client
            .post(SITEVERIFY_URL)
            .form(&form)
            .send()
            .await?
            .error_for_status()?;

        let body: SiteverifyResponse = resp.json().await?;
        if !body.success {
            log::warn!(
                "turnstile verification failed: {:?}",
                body.error_codes.unwrap_or_default()
            );
        }
        Ok(body.success)
    }
}

#[derive(Deserialize)]
struct SiteverifyResponse {
    success: bool,
    #[serde(rename = "error-codes")]
    error_codes: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct CaptchaConfigResponse {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    site_key: Option<String>,
}

/// tell the frontend whether to load Turnstile
pub async fn captcha_config(
    State(turnstile): State<Option<Arc<TurnstileVerifier>>>,
) -> Json<CaptchaConfigResponse> {
    match turnstile {
        Some(verifier) => Json(CaptchaConfigResponse {
            enabled: true,
            site_key: Some(verifier.site_key().to_owned()),
        }),
        None => Json(CaptchaConfigResponse {
            enabled: false,
            site_key: None,
        }),
    }
}
