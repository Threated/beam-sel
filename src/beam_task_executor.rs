use std::{collections::HashSet, time::Duration, net::SocketAddr};

use beam_lib::{BlockingOptions, SocketTask};
use tokio::net::TcpStream;
use tracing::{info, warn, debug};

use crate::{BEAM_CLIENT, CONFIG};

pub async fn beam_task_executor() {
    // TODO: Remove once feature/stream-tasks is merged
    let mut seen = HashSet::new();
    let block_one = BlockingOptions::from_count(1);
    loop {
        match BEAM_CLIENT.get_socket_tasks(&block_one).await {
            Ok(tasks) => tasks.into_iter().for_each(|task| {
                if !seen.contains(&task.id) {
                    seen.insert(task.id);
                    tokio::spawn(handle_task(task));
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

async fn handle_task(task: SocketTask) {
    let mut remote_socket = match BEAM_CLIENT.connect_socket(&task.id).await {
        Ok(socket) => socket,
        Err(e) => {
            warn!(?task, "Failed to connect to beam socket: {e}");
            return;
        }
    };
    let connect_addr = task.metadata
        .as_u64()
        .map(|port| SocketAddr::from((CONFIG.sel_addr.ip(), port as u16)))
        .unwrap_or(CONFIG.sel_addr);
    let mut local_socket = match TcpStream::connect(connect_addr).await {
        Ok(socket) => socket,
        Err(e) => {
            warn!(%CONFIG.sel_addr, "Failed to connect to sel socket: {e}");
            return;
        }
    };
    if let Err(e) = tokio::io::copy_bidirectional(&mut local_socket, &mut remote_socket).await {
        debug!("Relaying socket connection failed: {e}");
    }
}
