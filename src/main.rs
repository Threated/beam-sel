
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
use tracing::info;
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
    tokio::select!{
        server_res = server => {
            info!("Server shuttdown: {server_res:?}");
        },
        _ = beam_task_executor(state) => {}
    }
}

/// This is just a better bash script with a lot of hardcoded stuff
#[cfg(test)]
mod intergration_tests {
    use beam_lib::reqwest;
    use serde_json::Value;
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn normal_request() -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let body = "test1234";
        let res = client.get("http://localhost:8061")
            .header("beam-remote", "proxy2")
            .body(body)
            .send()
            .await?;
        assert_eq!(res.json::<Value>().await?["body"].as_str(), Some(body));
        Ok(())
    }

    #[tokio::test]
    async fn test_sel_negotiation() -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let body = "test1234";
        let res = client.get("http://localhost:8061/testConfig")
            .header("beam-remote", "proxy2")
            .header("SEL-Port", "81")
            .body(body)
            .send()
            .await?;
        assert_eq!(res.json::<Value>().await?["body"].as_str(), Some(body));
        // let stream = TcpStream::connect("")
        Ok(())
    }
}