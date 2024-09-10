use anyhow::bail;
use axum::{async_trait, body::Body, extract::{FromRequestParts, State}, http::{request::Parts, HeaderName, Request, StatusCode}, response::{IntoResponse, Response, Result}};
use beam_lib::{AppId, MsgId};
use futures_util::TryStreamExt;
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tokio::{io::{AsyncRead, AsyncWrite}, net::TcpListener, sync::oneshot};
use tokio_util::io::{ReaderStream, StreamReader};
use tracing::{debug, info, info_span, warn, Instrument, Span};

use crate::{AppState, BEAM_CLIENT, CONFIG};

static BEAM_REMOTE_HEADER: HeaderName = HeaderName::from_static("beam-remote");

pub struct ExtractRemote(pub AppId);

#[async_trait]
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

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub(crate) struct Intend {
    pub(crate) id: MsgId,
    pub(crate) port: Option<u16>,
}

impl Intend {
    pub(crate) fn http() -> Self {
        Self { id: MsgId::new(), port: None }
    }

    pub(crate) fn sel(port: u16) -> Self {
        Self { id: MsgId::new(), port: Some(port) }
    }
}

#[tracing::instrument(skip_all, fields(to = %remote, method = ?req.method(), path = ?req.uri().path()))]
pub async fn forward_request(
    ExtractRemote(remote): ExtractRemote,
    State(state): State<AppState>,
    req: Request<Body>
) -> Result<Response, StatusCode> {
    let is_sel_port_negotiation = req.uri().path().contains("/testConfig");

    let socket = negotiate_socket(remote.clone(), Intend::http(), &state).await.map_err(|e| {
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
        on_mpc_port_decided(port, remote, state).await;
    }
    info!("Successful request");
    Ok(res.into_response())
}

#[tracing::instrument(skip(state, remote))]
async fn on_mpc_port_decided(port: u16, remote: AppId, state: AppState) {
    let listener = match TcpListener::bind((CONFIG.bind_addr.ip(), port)).await {
        Ok(socket) => {
            info!("Binded to port {port}");
            socket
        },
        Err(e) => {
            warn!("Failed to bind to socket: {e}");
            return;
        }
    };
    let span = info_span!("Socket tunnel", port, to = remote.as_ref());
    tokio::spawn(async move {
        info!("Waiting for connection");
        let mut local_con = match listener.accept().await {
            Ok((socket, _addr)) => {
                info!("Starting socket relay");
                socket
            },
            Err(e) => {
                warn!("Failed accept on local socket: {e}");
                return;
            }
        };
        let mut remote = match negotiate_socket(remote, Intend::sel(port), &state).instrument(Span::current()).await {
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
    }.instrument(span));
}

#[tracing::instrument(skip(state, remote), fields(to = %remote.as_ref()))]
async fn negotiate_socket(remote: AppId, intend: Intend, state: &AppState) -> anyhow::Result<impl AsyncRead + AsyncWrite> {
    let (tx, rx) = oneshot::channel();
    state.lock().await.insert(intend.id, tx);
    let (local_read, local_write) = tokio::io::simplex(1024 * 4);
    let mut write_fut = Box::pin(async move {
        BEAM_CLIENT.create_socket_with_metadata(&remote, ReaderStream::new(local_read), intend).await
    });
    debug!("Waiting for write to connect");
    let remote_read = tokio::select! {
        recv_res = rx => {
            let Ok(remote_read) = recv_res else {
                bail!("Gone")
            };
            remote_read
        }
        write_done = &mut write_fut => {
            let res = write_done?;
            if res.status().is_success() {
                panic!("Hm");
            } else {
                bail!("Strange beam response: {res:?}")
            }
        }
    };
    tokio::spawn(async move {
        match write_fut.await {
            Ok(res) if res.status().is_success() => debug!("Successful write ended"),
            Ok(other) => warn!("Writer response had other status: {other:?}"),
            Err(e) => warn!(%e, "Beam connection failed"),
        }
    }.instrument(Span::current()));
    Ok(tokio::io::join(StreamReader::new(remote_read.map_err(std::io::Error::other)), local_write))
}