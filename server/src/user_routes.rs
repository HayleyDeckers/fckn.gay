use axum::extract::Json as AxumJson;
use axum::http::StatusCode;
use fckn_gay_dns::Record as DnsRecord;

async fn add_record_endpoint(
    State(dns): State<Arc<Mutex<Dns>>>,
    authenticed_for: AuthenticatedFor,
    AxumJson(req): AxumJson<DnsRecord>,
) -> Result<StatusCode, AppError> {
    // Only allow adding records for the authenticated user
    let user_pat = format!(".{}.is.fckn.gay", authenticed_for.user_id());
    if !req.name.ends_with(&user_pat) && req.name != user_pat[1..] {
        return Err(anyhow::anyhow!("Record name must match your user domain").into());
    }
    let _key = dns.lock().await.add_record(req).await?;
    Ok(StatusCode::CREATED)
}
use std::sync::Arc;

use crate::{
    auth_cache::{AuthenticatedFor, add_authorization_or_unauthorized, redirect_if_unauthorized},
    error::AppError,
};
use axum::{
    Router,
    extract::{Json, State},
    middleware::from_fn_with_state,
};
use fckn_gay_dns::{Dns, Interface as DnsInterface, Record};
use tokio::sync::Mutex;

async fn dns_records(
    State(dns): State<Arc<Mutex<Dns>>>,
    authenticed_for: AuthenticatedFor,
) -> Result<Json<Vec<Record>>, AppError> {
    let records = dns.lock().await.list_records().await?;
    let pat = format!(".{}.is.fckn.gay", authenticed_for.user_id());
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
    Ok(Json(filtered_records))
}

pub fn api_router(appstate: crate::Interfaces) -> Router<crate::Interfaces> {
    Router::new()
        .route("/records", axum::routing::get(dns_records))
        .route("/add_record", axum::routing::post(add_record_endpoint))
        .layer(from_fn_with_state(
            appstate.auth_cache.clone(),
            add_authorization_or_unauthorized,
        ))
        .with_state(appstate)
}

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
