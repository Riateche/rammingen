#![allow(clippy::collapsible_else_if)]

use std::{
    collections::HashMap, convert::Infallible, env, marker::PhantomData, net::SocketAddr,
    path::PathBuf, sync::Arc,
};

use anyhow::{anyhow, bail, Result};
use futures_util::{FutureExt, SinkExt, StreamExt, TryFutureExt, TryStreamExt};
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use hyper::{
    body::{self, Bytes, Frame},
    header::AUTHORIZATION,
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use rammingen_protocol::{RequestVariant, SourceId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sqlx::{query, PgPool};
use storage::Storage;
use stream_generator::generate_try_stream;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
use tracing::{info, warn};

use crate::handler::Handler;

pub mod handler;
pub mod storage;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    database_url: String,
    storage_path: PathBuf,
    bind_addr: SocketAddr,
}

#[derive(Debug, Clone)]
struct Context {
    db_pool: PgPool,
    storage: Arc<Storage>,
    sources: Arc<HashMap<String, SourceId>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::args().nth(1).expect("missing config arg");
    let config: Config = json5::from_str(&fs_err::read_to_string(config_path)?)?;
    let db_pool = PgPool::connect(&config.database_url).await?;
    let sources = query!("SELECT id, secret FROM sources")
        .fetch_all(&db_pool)
        .await?;

    let ctx = Context {
        db_pool: PgPool::connect(&config.database_url).await?,
        storage: Arc::new(Storage::new(config.storage_path)?),
        sources: Arc::new(
            sources
                .into_iter()
                .map(|row| (row.secret, SourceId(row.id)))
                .collect(),
        ),
    };

    tracing_subscriber::fmt::init();

    // Create the event loop and TCP listener we'll accept connections on.
    let listener = TcpListener::bind(&config.bind_addr).await?;
    info!("Listening on: {}", config.bind_addr);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .keep_alive(true)
                        .serve_connection(
                            stream,
                            service_fn(move |req| handle_request(ctx.clone(), req)),
                        )
                        .await
                    {
                        warn!(%err, "error while serving HTTP connection");
                    }
                });
            }
            Err(err) => warn!(%err, "failed to accept"),
        }
    }
}

async fn handle_request(
    ctx: Context,
    request: Request<body::Incoming>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
    try_handle_request(ctx, request).await.or_else(|code| {
        Ok(Response::builder()
            .status(code)
            .body(Full::new(Bytes::from(code.as_str().to_string())).boxed())
            .unwrap())
    })
}

async fn try_handle_request(
    ctx: Context,
    request: Request<body::Incoming>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, StatusCode> {
    let source_id = auth(&ctx, &request).map_err(|err| {
        warn!(%err, "auth error");
        StatusCode::UNAUTHORIZED
    })?;

    let ctx = handler::Context {
        db_pool: ctx.db_pool,
        storage: ctx.storage,
        source_id,
    };

    let path = request.uri().path();
    if path == "/entries" && request.method() == Method::GET {
        //let request = match bincode::deserialize(bytes)
        let (tx, rx) = mpsc::channel(5);
        let request = parse_request(request).await?;
        tokio::spawn(async move {
            if let Err(err) = handler::get_entries(ctx, request, tx.clone()).await {
                let _ = tx.send(Err(err)).await;
            }
        });

        Ok(Response::new(BodyExt::boxed(StreamBody::new(
            ReceiverStream::new(rx).map(serialize_response),
        ))))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn parse_request<T: DeserializeOwned>(
    request: Request<body::Incoming>,
) -> Result<T, StatusCode> {
    let bytes = request
        .into_body()
        .collect()
        .await
        .map_err(|err| {
            warn!(%err, "failed to read request body");
            StatusCode::BAD_REQUEST
        })?
        .to_bytes();
    bincode::deserialize(&bytes).map_err(|err| {
        warn!(%err, "failed to deserialize request body");
        StatusCode::BAD_REQUEST
    })
}

fn serialize_response<T: Serialize>(data: anyhow::Result<T>) -> Result<Frame<Bytes>, Infallible> {
    let data = bincode::serialize(&data.map_err(|err| {
        warn!(%err, "handler error");
        err.to_string()
    }))
    .expect("bincode serialization failed");
    Ok(Frame::data(data.into()))
}

fn auth(ctx: &Context, request: &Request<body::Incoming>) -> anyhow::Result<SourceId> {
    let auth = request
        .headers()
        .get(AUTHORIZATION)
        .ok_or_else(|| anyhow!("missing authorization header"))?
        .to_str()?;
    let secret = auth
        .strip_prefix("Bearer ")
        .ok_or_else(|| anyhow!("authorization header is not Bearer"))?;
    ctx.sources
        .get(secret)
        .copied()
        .ok_or_else(|| anyhow!("invalid bearer token"))
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
