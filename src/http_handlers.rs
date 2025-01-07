use axum::{body::Body, extract::FromRequestParts, http::{request::Parts, HeaderName, Request, StatusCode}, response::{IntoResponse, Response, Result}};
use beam_lib::AppId;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tracing::{debug, info, info_span, warn, Instrument, Span};

use crate::{BEAM_CLIENT, CONFIG};

static BEAM_REMOTE_HEADER: HeaderName = HeaderName::from_static("beam-remote");

pub struct ExtractRemote(pub AppId);

impl<S: Send + Sync> FromRequestParts<S> for ExtractRemote {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let beam_remote = parts.headers
            .get(&BEAM_REMOTE_HEADER)
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| {
                warn!(?parts.headers, "Did not find beam-remote header");
                StatusCode::BAD_REQUEST
            })?;
        Ok(Self(AppId::new_unchecked(format!(
            "{}.{}.{}",
            CONFIG.beam_id.app_name(),
            beam_remote,
            CONFIG.beam_id.proxy_id().as_ref().split_once('.').expect("Beam ID to be valid").1
        ))))
    }
}

#[tracing::instrument(skip_all, fields(to = %remote, method = ?req.method(), path = ?req.uri().path()))]
pub async fn forward_request(
    ExtractRemote(remote): ExtractRemote,
    req: Request<Body>
) -> Result<Response, StatusCode> {
    let is_sel_port_negotiation = req.uri().path().contains("/testConfig");

    let socket = BEAM_CLIENT.create_socket(&remote).await.map_err(|e| {
        warn!(%e, "Failed to negotiate socket");
        StatusCode::BAD_GATEWAY
    })?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(socket)).await.map_err(|e| {
        warn!("Failed http handshake: {e}");
        StatusCode::INTERNAL_SERVER_ERROR 
    })?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            warn!("Error in connection: {e}");
        }
    }.instrument(Span::current()));
    let res = sender.send_request(req).await.map_err(|e| {
        warn!("Failed to send request: {e}");
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
    info!("Successful request");
    Ok(res.into_response())
}

#[tracing::instrument]
async fn on_mpc_port_decided(port: u16, remote: AppId) {
    let listener = match TcpListener::bind((CONFIG.bind_addr.ip(), port)).await {
        Ok(socket) => {
            info!("Bound to port {port}");
            socket
        },
        Err(e) => {
            warn!("Failed to bind to socket: {e}");
            return;
        }
    };
    let span = info_span!(parent: None, "Socket relay", port, to = remote.as_ref());
    tokio::spawn(async move {
        info!("Waiting for connection");
        for i in 0..2 {
            let mut local_con = match listener.accept().await {
                Ok((socket, _addr)) => {
                    info!(%i, "Starting socket relay");
                    socket
                },
                Err(e) => {
                    warn!("Failed accept on local socket: {e}");
                    return;
                }
            };
            let remote = remote.clone();
            tokio::spawn(async move {
                let mut remote = match BEAM_CLIENT.create_socket_with_metadata(&remote, port).await {
                    Ok(socket) => socket,
                    Err(e) => {
                        warn!("Failed create remote socket: {e}");
                        return;
                    }
                };
                info!("Negotiated socket for relay. Transfering");
                if let Err(e) = tokio::io::copy_bidirectional(&mut local_con, &mut remote).await {
                    debug!("Relaying socket connection failed: {e}");
                }
                debug!("Transfer ended");
            }.instrument(info_span!("Relay", i)));
        }
    }.instrument(span));
}
