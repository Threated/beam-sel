use axum::{response::{Result, Response, IntoResponse}, http::{Request, StatusCode, request::Parts, HeaderName}, body::Body, extract::FromRequestParts, async_trait};
use beam_lib::AppId;
use tokio::net::TcpListener;
use tracing::{warn, debug};

use crate::{CONFIG, BEAM_CLIENT};

static BEAM_REMOTE_HEADER: HeaderName = HeaderName::from_static("beam-remote");

pub struct ExtractRemote(pub AppId);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for ExtractRemote {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let beam_remote = parts.headers
            .get(&BEAM_REMOTE_HEADER)
            .and_then(|h| h.to_str().ok())
            .ok_or(StatusCode::BAD_REQUEST)?;
        Ok(Self(AppId::new_unchecked(format!(
            "{}.{}.{}",
            CONFIG.beam_id.app_name(),
            beam_remote,
            CONFIG.beam_id.proxy_id().as_ref().split_once('.').expect("Beam ID to be valid").1
        ))))
    }
}

pub async fn forward_request(
    ExtractRemote(remote): ExtractRemote,
    req: Request<Body>
) -> Result<Response> {
    let is_sel_port_negotiation = req.uri().path().contains("/testConfig");

    // Connect socket and send request
    let remote_con = BEAM_CLIENT.create_socket(&remote).await.map_err(|e| {
        warn!("Failed to create socket connection to {remote}: {e}");
        StatusCode::INTERNAL_SERVER_ERROR 
    })?;
    let (mut sender, conn) = hyper::client::conn::handshake(remote_con).await.map_err(|e| {
        warn!("Failed handshake with {remote}: {e}");
        StatusCode::INTERNAL_SERVER_ERROR 
    })?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            warn!("Error in connection: {e}");
        }
    });
    let res = sender.send_request(req).await.map_err(|e| {
        warn!("Failed failed to send request to {remote}: {e}");
        StatusCode::INTERNAL_SERVER_ERROR 
    })?;
    if is_sel_port_negotiation {
        let port = res.headers()
            .get("SEL-Port")
            .and_then(|h| h.to_str().ok()?.parse().ok())
            .ok_or_else(|| {
                let headers = res.headers();
                warn!(?headers, "Failed to extract port from /tesConfig request");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        on_mpc_port_decided(port, remote).await;
    }
    Ok(res.into_response())
}

async fn on_mpc_port_decided(port: u16, remote: AppId) {
    let listener = match TcpListener::bind((CONFIG.bind_addr.ip(), port)).await {
        Ok(socket) => socket,
        Err(e) => {
            warn!("Failed to bind to socket: {e}");
            return;
        }
    };
    tokio::spawn(async move {
        // Wait for exactly one connection
        let mut local_con = match listener.accept().await {
            Ok((socket, _addr)) => socket,
            Err(e) => {
                warn!("Failed accept on local socket: {e}");
                return;
            }
        };
        let mut remote = match BEAM_CLIENT.create_socket_with_metadata(&remote, port).await {
            Ok(socket) => socket,
            Err(e) => {
                warn!("Failed accept on local socket: {e}");
                return;
            }
        };
        if let Err(e) = tokio::io::copy_bidirectional(&mut local_con, &mut remote).await {
            debug!("Relaying socket connection failed: {e}");
        }
    });
}
