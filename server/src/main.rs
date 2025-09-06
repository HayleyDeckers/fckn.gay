mod auth_cache;
mod error;
mod interfaces;
mod login;
mod user_routes;

use anyhow::{Context, Result};
use axum::routing::{get, post};
use interfaces::{Config, Interfaces};

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
    let user_routes = user_routes::router(interfaces.clone());
    let app = axum::Router::new()
        // .route("/", axum::routing::get(hello_world))
        .route("/login", post(login::login))
        .route("/logout", get(login::logout))
        .route("/sign-up", post(login::sign_up))
        .route("/confirm-sign-up/{uuid}", get(login::confirm_sign_up))
        .fallback_service(
            tower_http::services::ServeDir::new("server/static")
                .append_index_html_on_directories(true),
        )
        .nest("/u", user_routes)
        .with_state(interfaces);

    axum::serve(listener, app)
        .await
        .context("Failed to start server")?;

    Ok(())
}
