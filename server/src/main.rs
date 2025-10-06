mod auth_cache;
mod error;
mod interfaces;
mod login;
mod user_routes;

use std::any::Any;

use anyhow::{Context, Result};
use axum::{
    body::Body,
    response::Response,
    routing::{get, post},
};
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
    let config = Config::load_from_file("config.toml")?;
    let listener = tokio::net::TcpListener::bind(&config.address)
        .await
        .context("Failed to bind to address")?;
    println!("starting server on http://{}", config.address);
    let interfaces = Interfaces::new(config)?;

    // make a web server with axum
    // that uses the interfaces to glue all the functionality together
    // need one extra inteface for the auth cache
    // but perhaps we want to just integrate that with thhe user database
    //
    // we should also run a ctrl-c handler to gracefully shutdown the server
    // and flsuh the interfaces
    let user_routes = user_routes::router(interfaces.clone(), "server/static/u");
    let api_router = user_routes::api_router(interfaces.clone());
    let app = axum::Router::new()
        // frontend api routes
        .route("/login", post(login::login))
        .route("/logout", get(login::logout))
        .route("/sign-up", post(login::sign_up))
        .route("/confirm-sign-up/{uuid}", get(login::confirm_sign_up))
        // static files, /, favicon, css etc
        .fallback_service(
            tower_http::services::ServeDir::new("server/static")
                .append_index_html_on_directories(true),
        )
        // html pages that require authentication
        .nest("/user", user_routes)
        // api routes, most will need a valid session or api key
        .nest("/api", api_router)
        // Add panic-catching middleware to all routes with our silly error messages
        .layer(CatchPanicLayer::custom(silly_panic_handler))
        .with_state(interfaces);

    axum::serve(listener, app)
        .await
        .context("Failed to start server")?;

    Ok(())
}
