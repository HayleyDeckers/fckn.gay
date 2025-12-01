mod api;
mod auth;
mod auth_cache;
mod error;
mod interfaces;
mod rate_limit;
mod user_routes;

use std::{any::Any, net::SocketAddr};

use anyhow::{Context, Result};
use axum::{body::Body, response::Response};
use interfaces::{Config, Interfaces};
use tower_http::catch_panic::CatchPanicLayer;

/// Custom panic handler function that gives us those silly error messages 💀
fn silly_panic_handler(panic: Box<dyn Any + Send + 'static>) -> Response<Body> {
    // Extract the panic message if possible
    let panic_message = if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = panic.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "something went wrong".to_string()
    };

    // Create a silly error response that matches our style guide
    let body = format!(
        "server ded RIP 💀\n\n\
        The server had a little oopsie: {}\n\n\
        Don't worry, it's not your fault! The server is still running though, so you can try again.",
        panic_message
    );

    Response::builder()
        .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging - defaults to info level, override with RUST_LOG env var
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Config::load_from_file("config.toml")?;
    let listener = tokio::net::TcpListener::bind(&config.address)
        .await
        .context("Failed to bind to address")?;
    log::info!("starting server on http://{}", config.address);
    let interfaces = Interfaces::new(config)?;

    // Build rate limiting layers from config
    let auth_rate_limiter = rate_limit::auth_rate_limit_layer(&interfaces.rate_limit);
    let api_rate_limiter = rate_limit::api_rate_limit_layer(&interfaces.rate_limit);

    // make a web server with axum
    // that uses the interfaces to glue all the functionality together
    // need one extra inteface for the auth cache
    // but perhaps we want to just integrate that with thhe user database
    //
    // we should also run a ctrl-c handler to gracefully shutdown the server
    // and flsuh the interfaces
    let user_routes = user_routes::router(interfaces.clone(), "server/static/u");
    let api_router = api::router(interfaces.clone());
    let auth_router = auth::router(interfaces.clone());

    let app = auth_router
        // Rate limit auth routes by IP (prevents brute force attacks)
        .layer(auth_rate_limiter)
        // html pages that require authentication
        .merge(axum::Router::new().nest("/user", user_routes))
        // api routes with per-user rate limiting
        .merge(
            axum::Router::new()
                .nest("/api", api_router)
                .layer(api_rate_limiter),
        )
        // WASM files with correct MIME type
        .merge(axum::Router::new().route(
            "/fckn_gay_validation_bg.wasm",
            axum::routing::get(|| async {
                let wasm_data = std::fs::read("server/static/fckn_gay_validation_bg.wasm")
                    .unwrap_or_else(|_| Vec::new());
                Response::builder()
                    .header("content-type", "application/wasm")
                    .header("content-length", wasm_data.len())
                    .body(Body::from(wasm_data))
                    .unwrap()
            }),
        ))
        // static files, /, favicon, css etc
        .fallback_service(
            tower_http::services::ServeDir::new("server/static")
                .append_index_html_on_directories(true)
                .precompressed_gzip()
                .precompressed_br()
                .precompressed_deflate()
                .precompressed_zstd(),
        )
        // Add panic-catching middleware to all routes with our silly error messages
        .layer(CatchPanicLayer::custom(silly_panic_handler))
        .with_state(interfaces);

    // Use into_make_service_with_connect_info so SmartIpKeyExtractor can access peer IP
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("Failed to start server")?;

    Ok(())
}
