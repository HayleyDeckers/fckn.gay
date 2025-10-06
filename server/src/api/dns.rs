use std::sync::Arc;

use axum::{
    Router,
    extract::{Json as AxumJson, State},
    http::StatusCode,
    middleware::from_fn_with_state,
    routing::{get, post},
};
use fckn_gay_dns::{Dns, Interface as DnsInterface, Record as DnsRecord};
use tokio::sync::Mutex;

use crate::{
    auth_cache::{AuthenticatedFor, add_authorization_or_unauthorized},
    error::AppError,
    interfaces::PublicSuffix,
};

/// Get DNS records for the authenticated user
async fn get_records(
    State(dns): State<Arc<Mutex<Dns>>>,
    State(suffix): State<PublicSuffix>,
    authenticed_for: AuthenticatedFor,
) -> Result<axum::Json<Vec<DnsRecord>>, AppError> {
    let records = dns.lock().await.list_records().await?;
    let pat = format!(".{}{}", authenticed_for.user_id(), suffix);
    let filtered_records = records
        .into_iter()
        .filter_map(|(_, record)| {
            if record.name.ends_with(&pat) || record.name == pat[1..] {
                Some(record)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    Ok(axum::Json(filtered_records))
}

/// Add a new DNS record for the authenticated user
async fn add_record(
    State(dns): State<Arc<Mutex<Dns>>>,
    State(suffix): State<PublicSuffix>,
    authenticed_for: AuthenticatedFor,
    AxumJson(req): AxumJson<DnsRecord>,
) -> Result<StatusCode, AppError> {
    // Only allow adding records for the authenticated user
    let user_pat = format!(".{}{}", authenticed_for.user_id(), suffix);
    if !req.name.ends_with(&user_pat) && req.name != user_pat[1..] {
        return Err(anyhow::anyhow!("Record name must match your user domain").into());
    }
    let _key = dns.lock().await.add_record(req).await?;
    Ok(StatusCode::CREATED)
}

/// DNS API router with authentication middleware
pub fn router(appstate: crate::Interfaces) -> Router<crate::Interfaces> {
    Router::new()
        .route("/records", get(get_records))
        .route("/add_record", post(add_record))
        .layer(from_fn_with_state(
            appstate.auth_cache.clone(),
            add_authorization_or_unauthorized,
        ))
        .with_state(appstate)
}
