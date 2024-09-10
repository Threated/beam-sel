
use std::{collections::HashMap, sync::Arc};

use axum::{body::Bytes, Router};
use beam_lib::{BeamClient, MsgId};
use clap::Parser;
use config::Config;
use beam_task_executor::beam_task_executor;
use futures_util::stream::BoxStream;
use http_handlers::forward_request;
use once_cell::sync::Lazy;
use tokio::{net::TcpListener, sync::{oneshot, Mutex}};
use tracing_subscriber::{EnvFilter, util::SubscriberInitExt};

mod config;
mod beam_task_executor;
mod http_handlers;

pub static CONFIG: Lazy<Config> = Lazy::new(Config::parse);

pub type AppState = Arc<Mutex<HashMap<MsgId, oneshot::Sender<BoxStream<'static, Result<Bytes, beam_lib::reqwest::Error>>>>>>;

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
    let state = AppState::default();
    let server = axum::serve(
        TcpListener::bind(CONFIG.bind_addr).await.unwrap(),
        Router::new()
            .fallback(forward_request)
            .with_state(state.clone())
            .into_make_service(),
        )
        .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.unwrap() });
    tokio::join!(server, beam_task_executor(state)).0.unwrap();
}
