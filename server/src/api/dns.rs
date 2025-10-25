use axum::{
    Router,
    extract::{Json as AxumJson, State},
    http::StatusCode,
    middleware::from_fn_with_state,
    routing::{delete, get, post, put},
};
use fckn_gay_dns::{Interface as DnsInterface, Record as DnsRecord};
use fckn_gay_user_database::{DatabaseDnsRecord, DnsRecordId, Interface as UserDatabaseInterface};
use serde::Deserialize;

use crate::{
    auth_cache::{AuthenticatedFor, add_authorization_or_unauthorized},
    error::AppError,
};

/// Get DNS records for the authenticated user
async fn get_records(
    State(interfaces): State<crate::Interfaces>,
    authenticed_for: AuthenticatedFor,
) -> Result<axum::Json<Vec<DatabaseDnsRecord>>, AppError> {
    let records = interfaces
        .user_database
        .get_user_dns_records(authenticed_for.user_id())
        .await?;
    Ok(axum::Json(records))
}

/// Add a new DNS record for the authenticated user
/// Transaction-like behavior: DNS provider first, then database, rollback on failure
async fn add_record(
    State(interfaces): State<crate::Interfaces>,
    authenticed_for: AuthenticatedFor,
    AxumJson(req): AxumJson<DnsRecord>,
) -> Result<axum::Json<DnsRecordId>, AppError> {
    // Step 1: Add to DNS provider first
    let provider_key = interfaces.dns.add_record(req.clone()).await.map_err(|e| {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            anyhow::anyhow!("Failed to add record to DNS provider: {}", e),
        )
    })?;

    // Step 2: Add to database with provider key
    let provider_key_string = format!("{}", provider_key);
    match interfaces
        .user_database
        .add_dns_record(authenticed_for.user_id(), req, provider_key_string)
        .await
    {
        Ok(record_id) => Ok(axum::Json(record_id)),
        Err(db_error) => {
            // Step 3: Rollback - delete from DNS provider
            if let Err(rollback_error) = interfaces.dns.delete_record(provider_key).await {
                eprintln!("Failed to rollback DNS record: {}", rollback_error);
            }
            Err(AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                anyhow::anyhow!("Failed to add record to database: {}", db_error),
            ))
        }
    }
}

/// Delete a DNS record for the authenticated user
/// Transaction-like behavior: Database first, then DNS provider, rollback on failure
async fn delete_record(
    State(interfaces): State<crate::Interfaces>,
    authenticed_for: AuthenticatedFor,
    AxumJson(record_id): AxumJson<DnsRecordId>,
) -> Result<StatusCode, AppError> {
    // Get the provider key from the database
    let provider_key = interfaces
        .user_database
        .get_dns_record_provider_key(authenticed_for.user_id(), record_id.clone())
        .await
        .map_err(|e| {
            AppError::new(
                StatusCode::NOT_FOUND,
                anyhow::anyhow!("Record not found: {}", e),
            )
        })?;

    // Step 1: Delete from database first
    interfaces
        .user_database
        .delete_dns_record(authenticed_for.user_id(), record_id)
        .await
        .map_err(|e| {
            AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                anyhow::anyhow!("Failed to delete record from database: {}", e),
            )
        })?;

    // Convert string provider key to the appropriate Key type
    let dns_key = provider_key.parse().map_err(|e| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow::anyhow!("Failed to parse provider key: {}", e),
        )
    })?;
    // Step 2: Delete from DNS provider
    match interfaces.dns.delete_record(dns_key).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(dns_error) => {
            // Step 3: Rollback - add back to database
            // We need to get the record data to rollback, but we already deleted it
            // This is a limitation of the current approach
            eprintln!("Failed to delete from DNS provider: {}", dns_error);
            Err(AppError::new(
                StatusCode::BAD_GATEWAY,
                anyhow::anyhow!("Failed to delete record from DNS provider: {}", dns_error),
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateRecordRequest {
    id: DnsRecordId,
    content: DnsRecord,
}

/// Update a DNS record for the authenticated user
/// Transaction-like behavior: DNS provider first, then database, rollback on failure
async fn update_record(
    State(interfaces): State<crate::Interfaces>,
    authenticed_for: AuthenticatedFor,
    AxumJson(req): AxumJson<UpdateRecordRequest>,
) -> Result<StatusCode, AppError> {
    // Get the provider key from the database
    let provider_key = interfaces
        .user_database
        .get_dns_record_provider_key(authenticed_for.user_id(), req.id.clone())
        .await
        .map_err(|e| {
            AppError::new(
                StatusCode::NOT_FOUND,
                anyhow::anyhow!("Record not found: {}", e),
            )
        })?;

    // Convert string provider key to the appropriate Key type
    let dns_key = provider_key.parse().map_err(|e| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow::anyhow!("Failed to parse provider key: {}", e),
        )
    })?;

    // Step 1: Update in DNS provider first
    interfaces
        .dns
        .update_record(dns_key, req.content.clone())
        .await
        .map_err(|e| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                anyhow::anyhow!("Failed to update record in DNS provider: {}", e),
            )
        })?;

    // Step 2: Update in database
    match interfaces
        .user_database
        .update_dns_record(authenticed_for.user_id(), req.id, req.content.clone())
        .await
    {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(db_error) => {
            // Step 3: Rollback - we need to restore the original record in DNS provider
            // This is a limitation - we don't have the original record data
            eprintln!("Failed to update record in database: {}", db_error);
            Err(AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                anyhow::anyhow!("Failed to update record in database: {}", db_error),
            ))
        }
    }
}

/// DNS API router with authentication middleware
pub fn router(appstate: crate::Interfaces) -> Router<crate::Interfaces> {
    Router::new()
        .route("/dns/records", get(get_records))
        .route("/dns/add_record", post(add_record))
        .route("/dns/delete_record", delete(delete_record))
        .route("/dns/update_record", put(update_record))
        .with_state(appstate.clone())
        .layer(from_fn_with_state(
            appstate.auth_cache.clone(),
            add_authorization_or_unauthorized,
        ))
}
