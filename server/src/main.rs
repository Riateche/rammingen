use anyhow::{bail, Result};
use futures_util::{SinkExt, TryStreamExt};
use sqlx::PgPool;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::handler::Handler;

pub mod handler;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let addr = "127.0.0.1:8080".to_string();

    // Create the event loop and TCP listener we'll accept connections on.
    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on: {}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream).await {
                        warn!(%err, "handle_connection failed")
                    }
                });
            }
            Err(err) => warn!(%err, "failed to accept"),
        }
    }
}

async fn handle_connection(stream: TcpStream) -> Result<()> {
    let addr = stream.peer_addr()?;
    info!("new connection: {}", addr);

    let mut ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    info!("websocket connection established: {}", addr);

    let mut handler = Handler::new(PgPool::connect("todo").await?);

    while let Some(message) = ws_stream.try_next().await? {
        match message {
            Message::Binary(data) => {
                let request = bincode::deserialize(&data)?;
                let (response, is_ok) = handler.handle(request).await;
                ws_stream.send(Message::Binary(response)).await?;
                if !is_ok {
                    ws_stream.close(None).await?;
                    break;
                }
            }
            Message::Ping(ping) => {
                ws_stream.send(Message::Pong(ping)).await?;
            }
            Message::Text(_) => bail!("unexpected text message"),
            Message::Pong(_) | Message::Close(_) | Message::Frame(_) => {}
        }
    }

    info!("websocket connection terminated: {}", addr);
    Ok(())
}
