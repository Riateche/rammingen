#![allow(clippy::collapsible_else_if)]

mod content_streaming;
mod handler;
mod snapshot;
mod storage;
pub mod util;

use std::{
    cmp::min,
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    path::{Path, PathBuf},
    pin::pin,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use bytes::{BufMut, BytesMut};
use futures_util::{Future, StreamExt, TryStreamExt};
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use humantime_serde::re::humantime::parse_duration;
use hyper::{
    body::{self, Bytes, Frame},
    header::AUTHORIZATION,
    Method, Request, Response, StatusCode,
};
use rammingen_protocol::{
    endpoints::{
        AddVersions, CheckIntegrity, ContentHashExists, GetAllEntryVersions, GetDirectChildEntries,
        GetEntryVersionsAtTime, GetNewEntries, GetServerStatus, GetSources, MovePath, RemovePath,
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
    select,
    sync::{
        mpsc::{self, Sender},
        Mutex,
    },
    task,
    time::interval,
};
use tracing::{error, info, warn};

use rammingen_sdk::{
    server::{serve_connection, ShutdownWatcher},
    signal::shutdown_signal,
};

use crate::snapshot::make_snapshot;
use util::default_config_dir;

const SOURCES_CACHE_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub storage_path: PathBuf,
    pub bind_addr: SocketAddr,
    #[serde(default)]
    pub log_file: Option<PathBuf>,
    #[serde(default = "default_log_filter")]
    pub log_filter: String,

    #[serde(with = "humantime_serde", default = "default_snapshot_interval")]
    pub snapshot_interval: Duration,
    #[serde(
        with = "humantime_serde",
        default = "default_retain_detailed_history_for"
    )]
    pub retain_detailed_history_for: Duration,
}

fn default_snapshot_interval() -> Duration {
    parse_duration("1week").unwrap()
}

fn default_retain_detailed_history_for() -> Duration {
    parse_duration("1week").unwrap()
}

impl Config {
    pub fn parse(config_path: impl AsRef<Path>) -> Result<Self> {
        Ok(json5::from_str(&fs_err::read_to_string(config_path)?)?)
    }
}

fn default_log_filter() -> String {
    "info,sqlx::query=warn,rammingen_protocol=debug,rammingen_server=debug,rammingen_sdk=debug"
        .to_owned()
}

#[derive(Debug, Clone)]
pub struct Context {
    db_pool: PgPool,
    storage: Arc<Storage>,
    sources: Arc<Mutex<CachedSources>>,
    config: Config,
}

#[derive(Debug)]
struct CachedSources {
    sources: HashMap<String, SourceId>,
    updated_at: Instant,
}

async fn load_sources(db_pool: &PgPool) -> Result<HashMap<String, SourceId>> {
    query!("SELECT id, access_token FROM sources")
        .fetch(db_pool)
        .map_ok(|row| (row.access_token, row.id.into()))
        .try_collect()
        .await
        .map_err(Into::into)
}

pub async fn run(config: Config) -> Result<()> {
    info!("Connecting to database...");
    let db_pool = PgPool::connect(&config.database_url).await?;
    info!("Connected to database.");
    let ctx = Context {
        config: config.clone(),
        storage: Arc::new(Storage::new(config.storage_path)?),
        sources: Arc::new(Mutex::new(CachedSources {
            sources: load_sources(&db_pool).await?,
            updated_at: Instant::now(),
        })),
        db_pool,
    };

    let listener = TcpListener::bind(&config.bind_addr).await?;
    info!("Listening on {}", config.bind_addr);

    let snapshot_check_interval = min(config.snapshot_interval / 2, Duration::from_secs(60));
    let ctx2 = ctx.clone();
    task::spawn(async move {
        let mut interval = interval(snapshot_check_interval);
        loop {
            interval.tick().await;
            if let Err(err) = make_snapshot(&ctx2).await {
                error!(?err, "error while making snapshot");
            }
        }
    });

    let mut shutdown = pin!(shutdown_signal());
    let shutdown_watcher = ShutdownWatcher::default();
    loop {
        select! {
            r = listener.accept() => match r {
                Ok((io, _client_addr)) => {
                    let ctx = ctx.clone();
                    tokio::spawn(serve_connection(io, &shutdown_watcher, move |request| {
                        handle_request(ctx.clone(), request)
                    }));
                }
                Err(err) => warn!(?err, "failed to accept"),
            },
            signal = &mut shutdown => {
                info!(signal = %signal?, "shutting down");
                break;
            },
        }
    }
    shutdown_watcher.shutdown().await;
    Ok(())
}

async fn handle_request(
    ctx: Context,
    request: Request<body::Incoming>,
) -> Response<BoxBody<Bytes, Infallible>> {
    try_handle_request(ctx, request)
        .await
        .unwrap_or_else(|code| {
            Response::builder()
                .status(code)
                .body(Full::new(Bytes::from(code.as_str().to_string())).boxed())
                .expect("response builder failed")
        })
}

async fn try_handle_request(
    ctx: Context,
    request: Request<body::Incoming>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, StatusCode> {
    let source_id = auth(&ctx, &request).await.map_err(|err| {
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
    } else if path == AddVersions::PATH {
        wrap_request(ctx, request, handler::add_versions).await
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
    } else if path == CheckIntegrity::PATH {
        wrap_request(ctx, request, handler::check_integrity).await
    } else if path == GetSources::PATH {
        wrap_request(ctx, request, handler::get_sources).await
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
    Fut: Future<Output = Result<<T as RequestToResponse>::Response>>,
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
    F: FnOnce(handler::Context, T, Sender<Result<StreamingResponseItem<T>>>) -> Fut
        + Send
        + 'static,
    Fut: Future<Output = Result<()>> + Send,
{
    let (tx, mut rx) = mpsc::channel(5);
    let request = parse_request::<T>(request).await?;
    tokio::spawn(async move {
        if let Err(err) = f(ctx, request, tx.clone()).await {
            let _ = tx.send(Err(err)).await;
        }
    });

    let body_stream = generate_stream(move |mut y| async move {
        async fn send<T>(y: &mut Yielder<Bytes>, data: Result<Option<&[StreamingResponseItem<T>]>>)
        where
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

fn serialize_response<T: Serialize>(data: Result<T>) -> Bytes {
    bincode::serialize(&data.map_err(|err| {
        warn!(?err, "handler error");
        format!("{err:?}")
    }))
    .expect("bincode serialization failed")
    .into()
}

fn serialize_response_with_length<T: Serialize>(data: Result<T>) -> Bytes {
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

async fn auth(ctx: &Context, request: &Request<body::Incoming>) -> Result<SourceId> {
    let auth = request
        .headers()
        .get(AUTHORIZATION)
        .ok_or_else(|| anyhow!("missing authorization header"))?
        .to_str()?;
    let access_token = auth
        .strip_prefix("Bearer ")
        .ok_or_else(|| anyhow!("authorization header is not Bearer"))?;
    let mut sources = ctx.sources.lock().await;
    if sources.updated_at.elapsed() > SOURCES_CACHE_INTERVAL {
        sources.sources = load_sources(&ctx.db_pool).await?;
        sources.updated_at = Instant::now();
    }
    sources
        .sources
        .get(access_token)
        .copied()
        .ok_or_else(|| anyhow!("invalid bearer token"))
}

pub fn config_path(config: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = config {
        Ok(path)
    } else {
        Ok(default_config_dir()?.join("rammingen-server.conf"))
    }
}
