#![allow(clippy::collapsible_else_if)]

use std::{collections::HashMap, convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{anyhow, Result};
use bytes::{BufMut, BytesMut};
use futures_util::{Future, StreamExt};
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use hyper::{
    body::{self, Bytes, Frame},
    header::AUTHORIZATION,
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use rammingen_protocol::{
    endpoints::{
        AddVersion, ContentHashExists, GetAllEntryVersions, GetDirectChildEntries,
        GetEntryVersionsAtTime, GetNewEntries, GetServerStatus, MovePath, RemovePath,
        RequestToResponse, RequestToStreamingResponse, ResetVersion, StreamingResponseItem,
    },
    EncryptedContentHash, SourceId,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sqlx::{query, PgPool};
use storage::Storage;
use stream_generator::{generate_stream, Yielder};
use tokio::{
    net::TcpListener,
    sync::mpsc::{self, Sender},
};
use tracing::{info, warn};

mod content_streaming;
mod handler;
pub mod storage;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub storage_path: PathBuf,
    pub bind_addr: SocketAddr,
    pub log_file: Option<PathBuf>,
    #[serde(default = "default_log_filter")]
    pub log_filter: String,
}

fn default_log_filter() -> String {
    "info,sqlx::query=warn,rammingen_server=debug".into()
}

#[derive(Debug, Clone)]
struct Context {
    db_pool: PgPool,
    storage: Arc<Storage>,
    sources: Arc<HashMap<String, SourceId>>,
}

pub async fn run(config: Config) -> Result<()> {
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
                        warn!(?err, "error while serving HTTP connection");
                    }
                });
            }
            Err(err) => warn!(?err, "failed to accept"),
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
            .expect("response builder failed"))
    })
}

async fn try_handle_request(
    ctx: Context,
    request: Request<body::Incoming>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, StatusCode> {
    let source_id = auth(&ctx, &request).map_err(|err| {
        warn!(?err, "auth error");
        StatusCode::UNAUTHORIZED
    })?;

    let ctx = handler::Context {
        db_pool: ctx.db_pool,
        storage: ctx.storage,
        source_id,
    };

    let path = request.uri().path();
    if let Some(hash) = path.strip_prefix("/content/") {
        let hash = EncryptedContentHash::from_url_safe(hash).map_err(|err| {
            warn!(?err, "invalid hash");
            StatusCode::BAD_REQUEST
        })?;
        if request.method() == Method::PUT {
            content_streaming::upload(ctx, request, &hash).await
        } else if request.method() == Method::GET {
            content_streaming::download(ctx, &hash).await
        } else {
            Err(StatusCode::NOT_FOUND)
        }
    } else if request.method() != Method::POST {
        Err(StatusCode::NOT_FOUND)
    } else if path == GetNewEntries::PATH {
        wrap_stream(ctx, request, handler::get_new_entries).await
    } else if path == GetDirectChildEntries::PATH {
        wrap_stream(ctx, request, handler::get_direct_child_entries).await
    } else if path == GetEntryVersionsAtTime::PATH {
        wrap_stream(ctx, request, handler::get_entry_versions_at_time).await
    } else if path == GetAllEntryVersions::PATH {
        wrap_stream(ctx, request, handler::get_all_entry_versions).await
    } else if path == AddVersion::PATH {
        wrap_request(ctx, request, handler::add_version).await
    } else if path == MovePath::PATH {
        wrap_request(ctx, request, handler::move_path).await
    } else if path == RemovePath::PATH {
        wrap_request(ctx, request, handler::remove_path).await
    } else if path == ResetVersion::PATH {
        wrap_request(ctx, request, handler::reset_version).await
    } else if path == ContentHashExists::PATH {
        wrap_request(ctx, request, handler::content_hash_exists).await
    } else if path == GetServerStatus::PATH {
        wrap_request(ctx, request, handler::get_server_status).await
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn wrap_request<T, F, Fut>(
    ctx: handler::Context,
    request: Request<body::Incoming>,
    f: F,
) -> Result<Response<BoxBody<Bytes, Infallible>>, StatusCode>
where
    T: RequestToResponse + DeserializeOwned,
    <T as RequestToResponse>::Response: Serialize,
    F: FnOnce(handler::Context, T) -> Fut,
    Fut: Future<Output = anyhow::Result<<T as RequestToResponse>::Response>>,
{
    let request = parse_request(request).await?;
    let response = f(ctx, request).await;
    Ok(Response::new(BodyExt::boxed(Full::new(
        serialize_response(response),
    ))))
}

const ITEMS_PER_CHUNK: usize = 1024;

async fn wrap_stream<F, Fut, T>(
    ctx: handler::Context,
    request: Request<body::Incoming>,
    f: F,
) -> Result<Response<BoxBody<Bytes, Infallible>>, StatusCode>
where
    T: RequestToStreamingResponse + DeserializeOwned + Send + 'static,
    StreamingResponseItem<T>: Serialize + Send + Sync,
    F: FnOnce(handler::Context, T, Sender<anyhow::Result<StreamingResponseItem<T>>>) -> Fut
        + Send
        + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    type ChunkData<'a, T> = Option<&'a [StreamingResponseItem<T>]>;

    let (tx, mut rx) = mpsc::channel(5);
    let request = parse_request::<T>(request).await?;
    tokio::spawn(async move {
        if let Err(err) = f(ctx, request, tx.clone()).await {
            let _ = tx.send(Err(err)).await;
        }
    });

    let body_stream = generate_stream(move |mut y| async move {
        async fn send<T>(
            y: &mut Yielder<Bytes>,
            data: anyhow::Result<Option<&[StreamingResponseItem<T>]>>,
        ) where
            T: RequestToStreamingResponse,
            StreamingResponseItem<T>: Serialize,
        {
            y.send(serialize_response_with_length(data)).await;
        }

        let mut buf = Vec::new();
        while let Some(item) = rx.recv().await {
            match item {
                Ok(item) => {
                    buf.push(item);
                    if buf.len() >= ITEMS_PER_CHUNK {
                        send::<T>(&mut y, Ok(Some(&buf))).await;
                        buf.clear();
                    }
                }
                Err(err) => {
                    send::<T>(&mut y, Err(err)).await;
                    return;
                }
            }
        }
        if !buf.is_empty() {
            send::<T>(&mut y, Ok(Some(&buf))).await;
        }
        send::<T>(&mut y, Ok(None)).await;
    });

    Ok(Response::new(BodyExt::boxed(StreamBody::new(
        body_stream.map(|bytes| Ok(Frame::data(bytes))),
    ))))
}

async fn parse_request<T: DeserializeOwned>(
    request: Request<body::Incoming>,
) -> Result<T, StatusCode> {
    let bytes = request
        .into_body()
        .collect()
        .await
        .map_err(|err| {
            warn!(?err, "failed to read request body");
            StatusCode::BAD_REQUEST
        })?
        .to_bytes();
    bincode::deserialize(&bytes).map_err(|err| {
        warn!(?err, "failed to deserialize request body");
        StatusCode::BAD_REQUEST
    })
}

fn serialize_response<T: Serialize>(data: anyhow::Result<T>) -> Bytes {
    bincode::serialize(&data.map_err(|err| {
        warn!(?err, "handler error");
        format!("{err:?}")
    }))
    .expect("bincode serialization failed")
    .into()
}

fn serialize_response_with_length<T: Serialize>(data: anyhow::Result<T>) -> Bytes {
    let mut buf = BytesMut::zeroed(4);
    bincode::serialize_into(
        (&mut buf).writer(),
        &data.map_err(|err| {
            warn!(?err, "handler error");
            format!("{err:?}")
        }),
    )
    .expect("bincode serialization failed");
    let len = (buf.len() - 4) as u32;
    buf[0..4].copy_from_slice(&len.to_le_bytes());
    buf.freeze()
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

pub async fn migrate(db_url: &str) -> anyhow::Result<()> {
    let pool = PgPool::connect(db_url).await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(())
}
