mod api;
mod auth;
mod auth_cache;
mod captcha;
mod cli_commands;
mod error;
mod extract;
mod interfaces;
mod rate_limit;
mod telemetry;
mod user_routes;

use std::{any::Any, net::SocketAddr, panic, path::PathBuf};

use anyhow::{Context, Result};
use axum::{body::Body, response::Response};
use clap::Parser;
use interfaces::{Config, Interfaces};
use tower_http::{catch_panic::CatchPanicLayer, trace::TraceLayer};

/// CatchPanicLayer handler — just returns the JSON response.
/// Actual panic logging happens in the panic hook set in main().
fn panic_response(_panic: Box<dyn Any + Send + 'static>) -> Response<Body> {
    Response::builder()
        .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"error":"server ded RIP 💀"}"#))
        .unwrap()
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to config file
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,
    /// Use in-memory dummy providers for everything (ignores --config).
    #[arg(long)]
    dummy: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Import users + DNS records from the upstream DNS provider into the local database.
    /// Prompts for credentials for each new user found upstream. Will ask to delete users that are no longer present in the upstream DNS provider.
    Migrate {
        /// Don't actually create users or import records -- just show what would happen.
        #[arg(long)]
        dry_run: bool,
    },
    /// Edit an existing user's fields (email, state, password, username).
    /// Does not block the user from making racing changes while running this! use with care
    EditUser {
        /// Username of the user to edit
        username: String,
        /// Set a new email address
        #[arg(long)]
        email: Option<String>,
        /// Set account state (pending, active, inactive, banned)
        #[arg(long)]
        state: Option<fckn_gay_user_database::UserState>,
        /// Reset password to a random one (prints the new password)
        #[arg(long)]
        reset_password: bool,
        /// Rename the user (updates username + all DNS records)
        #[arg(long)]
        rename: Option<String>,
        /// Dry run -- don't actually change anything
        #[arg(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let config = if args.dummy {
        Config::dummy()
    } else {
        Config::load_from_file(&args.config)?
    };

    telemetry::logging::SubscriberBuilder::new(config.logging.clone()).init();

    // Log panics with tracing so they get span context + structured fields
    panic::set_hook(Box::new(|panic_info| {
        let panic_message = panic_info
            .payload_as_str()
            .unwrap_or("[non-string panic payload]");
        if let Some(location) = panic_info.location() {
            tracing::error!(panic.location = %location, "PANIC: {panic_message}");
        } else {
            tracing::error!("PANIC: {panic_message}");
        }
    }));

    if args.dummy {
        tracing::info!(
            "--dummy mode: using in-memory providers for everything, ignoring config file"
        );
    }

    let address = config.address.clone();
    let interfaces = Interfaces::new(config)?;

    match args.command {
        Some(Command::Migrate { dry_run }) => {
            cli_commands::migrate::run(
                &interfaces.dns,
                &interfaces.user_database,
                &interfaces.hostname,
                dry_run,
            )
            .await?;
        }
        Some(Command::EditUser {
            username,
            email,
            state,
            reset_password,
            rename,
            dry_run,
        }) => {
            cli_commands::edit_user::run(
                &interfaces.dns,
                &interfaces.user_database,
                &interfaces.hostname,
                &username,
                cli_commands::edit_user::EditUserOpts {
                    email,
                    state,
                    reset_password,
                    rename,
                    dry_run,
                },
            )
            .await?;
        }
        None => {
            let listener = tokio::net::TcpListener::bind(&address)
                .await
                .context("Failed to bind to address")?;
            tracing::info!("starting server on http://{address}");

            let trace_layer = TraceLayer::new_for_http()
                .make_span_with(telemetry::tracing_setup::MakeRequestSpan)
                .on_request(())
                .on_response(telemetry::tracing_setup::RecordStatusOnResponse)
                .on_failure(());

            // Each module owns its middleware (auth, rate limiting, etc).
            // main.rs just wires the routers together.
            let app = auth::router(interfaces.clone())
                .merge(axum::Router::new().nest(
                    "/user",
                    user_routes::router(interfaces.clone(), "server/static/u"),
                ))
                .merge(axum::Router::new().nest("/api", api::router(interfaces.clone())))
                .route(
                    "/api/captcha-config",
                    axum::routing::get(captcha::captcha_config),
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
                .layer(CatchPanicLayer::custom(panic_response))
                .layer(trace_layer)
                .with_state(interfaces);

            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .context("Failed to start server")?;
        }
    }

    Ok(())
}
