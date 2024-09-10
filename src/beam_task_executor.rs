use std::{collections::HashSet, time::Duration};

use beam_lib::{BlockingOptions, SocketTask};
use futures_util::TryStreamExt;
use tokio::net::TcpStream;
use tokio_util::io::{ReaderStream, StreamReader};
use tracing::{debug, field::debug, info, instrument, warn, Span};

use crate::{http_handlers::Intend, AppState, BEAM_CLIENT, CONFIG};

pub async fn beam_task_executor(state: AppState) {
    // TODO: Remove once feature/stream-tasks is merged
    let mut seen = HashSet::new();
    let block_one = BlockingOptions::from_count(1);
    loop {
        match BEAM_CLIENT.get_socket_tasks(&block_one).await {
            Ok(tasks) => tasks.into_iter().for_each(|task| {
                if !seen.contains(&task.id) {
                    seen.insert(task.id);
                    tokio::spawn(handle_task(task, state.clone()));
                }
            }),
            Err(beam_lib::BeamError::ReqwestError(e)) if e.is_connect() => {
                info!(
                    "Failed to connect to beam proxy on {}. Retrying in 30s",
                    CONFIG.beam_url
                );
                tokio::time::sleep(Duration::from_secs(30)).await
            }
            Err(e) => {
                warn!("Error during task polling {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[instrument(skip_all, fields(from = %task.from, meta))]
async fn handle_task(task: SocketTask, state: AppState) {
    let remote_write = match BEAM_CLIENT.connect_socket(&task.id).await {
        Ok(socket) => socket,
        Err(e) => {
            warn!(?task, "Failed to connect to beam socket: {e}");
            return;
        }
    };
    let meta: Intend = match serde_json::from_value(task.metadata) {
        Ok(v) => v,
        Err(e) => {
            warn!(%e, "Failed to deserialize socket metadata");
            return;
        }
    };
    Span::current().record("meta", debug(&meta));
    // If we find the id it means *we* asked for the remote to reply so we send the reply stream over 
    if let Some(sender) = state.lock().await.remove(&meta.id) {
        if sender.send(Box::pin(remote_write)).is_err() {
            warn!("Receiver of response stream was dropped");
        }
        info!("Successfully send over socket for id {}", meta.id);
        return;
    }
    // Otherwise connect to either the given SEL port or the REST endpoint of the SEL
    let connect_addr = meta.port.map_or(CONFIG.sel_addr, |p| (CONFIG.sel_addr.ip(), p).into());
    let sel_con = match TcpStream::connect(connect_addr).await {
        Ok(socket) => socket,
        Err(e) => {
            warn!(%CONFIG.sel_addr, "Failed to connect to sel socket: {e}");
            return;
        }
    };
    info!("Connected to {connect_addr}");
    let (r, mut w) = sel_con.into_split();
    let sel_read_to_remote_fut = BEAM_CLIENT.create_socket_with_metadata(&task.from, ReaderStream::new(r), meta);
    let mut stream_reader = StreamReader::new(remote_write.map_err(std::io::Error::other));
    let write_remote_data_to_sel_fut = tokio::io::copy(&mut stream_reader, &mut w);
    match tokio::join!(sel_read_to_remote_fut, write_remote_data_to_sel_fut) {
        (Err(e), _) => warn!(%e, "Failed to reply to socket task"),
        (_, Err(e)) => warn!(%e, "Failed to write to local sel"),
        _ => debug!("Successful reply"),
    };
}
