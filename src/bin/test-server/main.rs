use std::time::Duration;

use axum::{extract::Request, response::Response, Router};
use tokio::{io::{AsyncReadExt as _, AsyncWriteExt as _}, net::TcpListener};


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let http_echo = axum::serve(TcpListener::bind("0.0.0.0:80").await?, Router::new().fallback(echo));
    let tcp_echo = tcp_echo();
    tokio::select!(r = http_echo => r?, r = tcp_echo => r?);
    Ok(())
}

async fn tcp_echo() -> anyhow::Result<()> {
    let listen = TcpListener::bind("0.0.0.0:81").await?;
    while let Ok((mut socket, _)) = listen.accept().await {
        tokio::spawn(async move {
            let mut buf = vec![0; 1024];

            // In a loop, read data from the socket and write the data back.
            loop {
                let n = socket
                    .read(&mut buf)
                    .await
                    .expect("failed to read data from socket");

                if n == 0 {
                    return;
                }
                socket
                    .write_all(&buf[0..n])
                    .await
                    .expect("failed to write data to socket");
                println!("Wrote {n} bytes");
            }
        });
    }
    println!("Did this exit?");
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok(())
}

async fn echo(req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let mut res = Response::new(body);
    *res.headers_mut() = parts.headers;
    res
}