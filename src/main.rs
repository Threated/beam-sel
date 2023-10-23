
use axum::Router;
use beam_lib::BeamClient;
use clap::Parser;
use config::Config;
use beam_task_executor::beam_task_executor;
use http_handlers::forward_request;
use once_cell::sync::Lazy;
use tracing_subscriber::{EnvFilter, util::SubscriberInitExt};

mod config;
mod beam_task_executor;
mod http_handlers;

pub static CONFIG: Lazy<Config> = Lazy::new(Config::parse);

pub static BEAM_CLIENT: Lazy<BeamClient> = Lazy::new(|| BeamClient::new(
    &CONFIG.beam_id,
    &CONFIG.beam_secret,
    CONFIG.beam_url.clone()
));

#[tokio::main]
pub async fn main() {
    tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish()
        .init();
    let server = axum::Server::bind(&CONFIG.bind_addr)
        .serve(
            Router::new()
                // .route("/config", post(post_config))
                // .route(
                //     "/hello/:name",
                //     get(greet),
                // )
                .fallback(forward_request)
                .into_make_service(),
        )
        .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.unwrap() });
    tokio::join!(server, beam_task_executor()).0.unwrap();
}


