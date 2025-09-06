use std::sync::Arc;

use crate::auth_cache::{AuthenticatedFor, add_authorization_or_redirect};
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
) -> Result<Json<Vec<Record>>, String> {
    let records = dns
        .lock()
        .await
        .list_records()
        .await
        .map_err(|e| format!("Failed to list DNS records: {}", e))?; //todo(hayley): this is still a 200!
    let pat = format!(".{}.is.fckn.gay", authenticed_for.user_id());
    let filtered_records = records
        .into_iter()
        .filter_map(|(_, record)| {
            if record.name.starts_with(&pat) || record.name == pat[1..] {
                Some(record)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    Ok(Json(filtered_records))
}

pub fn router(appstate: crate::Interfaces) -> Router<crate::Interfaces> {
    let serve_dir = tower_http::services::ServeDir::new("server/static/user");
    Router::new()
        .route("/records", axum::routing::get(dns_records))
        // serve static files from the user directory
        .fallback_service(serve_dir)
        // all these routes require authentication to view
        // so we add the middleware here that converts the login-token cookie
        // to an authenticated user id
        .layer(from_fn_with_state(
            appstate.auth_cache.clone(),
            add_authorization_or_redirect,
        ))
        .with_state(appstate)
}
