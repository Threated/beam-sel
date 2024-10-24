use axum::Router;
use beam_lib::BeamClient;
use clap::Parser;
use config::Config;
use beam_task_executor::beam_task_executor;
use http_handlers::forward_request;
use once_cell::sync::Lazy;
use tokio::net::TcpListener;
use tracing::info;
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
        .without_time()
        .pretty()
        .with_file(false)
        .finish()
        .init();
    let server = axum::serve(
        TcpListener::bind(CONFIG.bind_addr).await.unwrap(),
        Router::new()
            .fallback(forward_request)
            .into_make_service(),
        )
        .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.unwrap() });
    tokio::select!{
        server_res = server => {
            info!("Server shutdown: {server_res:?}");
        },
        _ = beam_task_executor() => {}
    }
}

/// This is just a better bash script with a lot of hardcoded stuff
#[cfg(test)]
mod integration_tests {
    use beam_lib::reqwest;
    use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpStream};

    #[tokio::test]
    async fn normal_request() -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let body = "test1234";
        let res = client.get("http://localhost:8061")
            .header("beam-remote", "proxy2")
            .body(body)
            .send()
            .await?;
        assert!(res.status().is_success());
        assert_eq!(res.text().await?, body);
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
        assert_eq!(res.text().await?, body);
        for _ in 0..2 {
            let mut stream = TcpStream::connect("localhost:81").await?;
            stream.write_all(body.as_bytes()).await?;
            stream.flush().await?;
            let mut buf = vec![0; body.as_bytes().len()];
            stream.read_exact(&mut buf).await?;
            assert_eq!(String::from_utf8(buf)?, body);
        }
        Ok(())
    }
}