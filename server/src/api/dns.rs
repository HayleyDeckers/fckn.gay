use std::sync::Arc;

use axum::{
    Router,
    extract::{Json as AxumJson, Path, State},
    http::StatusCode,
    middleware::from_fn_with_state,
    routing::{delete, get, post},
};
use fckn_gay_dns::{Dns, Interface as DnsInterface, Record as DnsRecord};
use tokio::sync::Mutex;

use crate::{
    auth_cache::{AuthenticatedFor, add_authorization_or_unauthorized},
    error::AppError,
    interfaces::PublicSuffix,
};

/// A DNS record with key for frontend operations
#[derive(serde::Serialize)]
pub struct RecordWithKey {
    pub key: fckn_gay_dns::Key, // Key for deletion
    pub record: DnsRecord,
}

/// Get DNS records for the authenticated user
async fn get_records(
    State(dns): State<Arc<Mutex<Dns>>>,
    State(suffix): State<PublicSuffix>,
    authenticed_for: AuthenticatedFor,
) -> Result<axum::Json<Vec<RecordWithKey>>, AppError> {
    let records = dns.lock().await.list_records().await?;
    let pat = format!(".{}{}", authenticed_for.user_id(), suffix);
    let filtered_records = records
        .into_iter()
        .filter_map(|(key, record)| {
            if record.name.ends_with(&pat) || record.name == pat[1..] {
                Some(RecordWithKey { key, record })
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

/// Delete a DNS record by key (UUID-based deletion)
async fn delete_record_by_key(
    State(dns): State<Arc<Mutex<Dns>>>,
    State(suffix): State<PublicSuffix>,
    authenticed_for: AuthenticatedFor,
    Path(key_str): Path<String>,
) -> Result<StatusCode, AppError> {
    let key: fckn_gay_dns::Key =
        serde_json::from_str(&key_str).map_err(|e| anyhow::anyhow!("Invalid key format: {}", e))?;

    // Verify the record exists and is owned by the user
    let records = dns.lock().await.list_records().await?;
    let pat = format!(".{}{}", authenticed_for.user_id(), suffix);
    let user_records: Vec<_> = records
        .iter()
        .filter_map(|(k, record)| {
            if record.name.ends_with(&pat) || record.name == pat[1..] {
                Some(k)
            } else {
                None
            }
        })
        .collect();

    // Check if any user record has this key
    let has_key = user_records.iter().any(|k| **k == key);

    if !has_key {
        return Err(anyhow::anyhow!("Record not found or not owned by user").into());
    }

    dns.lock().await.delete_record_by_uuid(key).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a DNS record by full record matching
async fn delete_record_by_match(
    State(dns): State<Arc<Mutex<Dns>>>,
    State(suffix): State<PublicSuffix>,
    authenticed_for: AuthenticatedFor,
    AxumJson(record): AxumJson<DnsRecord>,
) -> Result<StatusCode, AppError> {
    // Verify the record exists and is owned by the user
    let records = dns.lock().await.list_records().await?;
    let pat = format!(".{}{}", authenticed_for.user_id(), suffix);
    let user_records: Vec<_> = records
        .iter()
        .filter_map(|(_, r)| {
            if r.name.ends_with(&pat) || r.name == pat[1..] {
                Some(r)
            } else {
                None
            }
        })
        .collect();

    // Check if the record exists in user's records
    let record_exists = user_records.iter().any(|r| {
        r.name == record.name
            && r.record_type == record.record_type
            && r.content == record.content
            && r.ttl_seconds == record.ttl_seconds
            && r.priority == record.priority
    });

    if !record_exists {
        return Err(anyhow::anyhow!("Record not found or not owned by user").into());
    }

    dns.lock().await.delete_record_by_match(record).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// DNS API router with authentication middleware
pub fn router(appstate: crate::Interfaces) -> Router<crate::Interfaces> {
    Router::new()
        .route("/records", get(get_records))
        .route("/add_record", post(add_record))
        .route("/delete_record_by_key/:key", delete(delete_record_by_key))
        .route("/delete_record", delete(delete_record_by_match))
        .layer(from_fn_with_state(
            appstate.auth_cache.clone(),
            add_authorization_or_unauthorized,
        ))
        .with_state(appstate)
}
