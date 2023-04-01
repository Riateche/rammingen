#![allow(clippy::collapsible_else_if)]

use std::{env, marker::PhantomData, path::PathBuf};

use anyhow::{bail, Result};
use futures_util::{SinkExt, TryStreamExt};
use rammingen_protocol::RequestVariant;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
use tracing::{info, warn};

use crate::handler::Handler;

pub mod handler;
pub mod storage;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    database_url: String,
    storage_path: PathBuf,
}

struct Context {
    db_pool: PgPool,
    storage_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::args().nth(1).expect("missing config arg");
    let config: Config = json5::from_str(&fs_err::read_to_string(config_path)?)?;
    let db_pool = PgPool::connect(&config.database_url).await?;

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

    let mut closed = false;
    while let Some(message) = ws_stream.try_next().await? {
        match message {
            Message::Binary(data) => {
                let request = bincode::deserialize(&data)?;
                let mut ws_handle = WsHandle::new(&mut ws_stream, &mut closed);
                handler.handle(request, &mut ws_handle).await?;
                if closed {
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

pub struct WsHandle<'a, T> {
    stream: &'a mut WebSocketStream<TcpStream>,
    closed: &'a mut bool,
    phantom: PhantomData<T>,
}

impl<'a> WsHandle<'a, ()> {
    fn new(stream: &'a mut WebSocketStream<TcpStream>, closed: &'a mut bool) -> Self {
        Self {
            stream,
            closed,
            phantom: PhantomData,
        }
    }
}

impl<'a, T: Serialize> WsHandle<'a, T> {
    fn for_request<Req: RequestVariant>(
        &mut self,
    ) -> WsHandle<'_, <Req as RequestVariant>::Response> {
        WsHandle {
            stream: self.stream,
            closed: self.closed,
            phantom: PhantomData,
        }
    }

    async fn send(&mut self, message: &Result<T>) -> Result<()> {
        let data = bincode::serialize(&message.as_ref().map_err(|e| e.to_string()))
            .expect("bincode serialization failed");
        self.stream.send(Message::Binary(data)).await?;
        Ok(())
    }
    async fn close(&mut self) -> Result<()> {
        *self.closed = true;
        self.stream.close(None).await?;
        Ok(())
    }
}
