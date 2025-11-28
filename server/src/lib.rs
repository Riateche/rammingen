#![allow(clippy::collapsible_else_if)]

mod content_streaming;
mod handler;
mod snapshot;
mod storage;
pub mod util;

use {
    crate::snapshot::make_snapshot,
    anyhow::{Context as _, Result},
    bytes::{BufMut, BytesMut},
    cadd::prelude::IntoType,
    futures::{Future, StreamExt, TryStreamExt},
    http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody},
    humantime_serde::re::humantime::parse_duration,
    hyper::{
        body::{self, Bytes, Frame},
        header::AUTHORIZATION,
        Method, Request, Response, StatusCode,
    },
    rammingen_protocol::{
        encoding,
        endpoints::{
            AddVersions, CheckIntegrity, ContentHashExists, GetAllEntryVersions,
            GetDirectChildEntries, GetEntryVersionsAtTime, GetNewEntries, GetServerStatus,
            GetSources, MovePath, RemovePath, RequestToResponse, RequestToStreamingResponse,
            ResetVersion, StreamingResponseItem,
        },
        EncryptedContentHash, SourceId,
    },
    rammingen_sdk::{
        server::{serve_connection, ShutdownWatcher},
        signal::shutdown_signal,
    },
    serde::{de::DeserializeOwned, Deserialize, Serialize},
    sqlx::{query, PgPool},
    std::{
        cmp::min,
        collections::HashMap,
        convert::Infallible,
        net::SocketAddr,
        path::{Path, PathBuf},
        pin::pin,
        sync::Arc,
        time::{Duration, Instant},
    },
    storage::Storage,
    stream_generator::{generate_stream, Yielder},
    tokio::{
        net::TcpListener,
        select,
        sync::{
            mpsc::{self, Sender},
            Mutex,
        },
        task,
        time::interval,
    },
    tracing::{error, info, warn},
};

/// Time between reloading the list of sources from the database.
const SOURCES_CACHE_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// URL of the database, e.g.
    /// "postgres://$DB_USER:$DB_PASSWORD@$DB_HOST:$DB_PORT/$DB_NAME"
    pub database_url: String,
    /// Path to the local file storage.
    pub storage_path: PathBuf,
    /// IP and port that the server will listen.
    pub bind_addr: SocketAddr,
    /// Path to the log file. If not specified, log will be written to stdout.
    pub log_file: Option<PathBuf>,
    /// Log filter in [tracing format](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html).
    #[serde(default = "default_log_filter")]
    pub log_filter: String,
    /// Time between snapshots. A snapshot is a copy of the state of all archive entries at a certain time.
    /// Snapshots are not deleted automatically.
    #[serde(with = "humantime_serde", default = "default_snapshot_interval")]
    pub snapshot_interval: Duration,
    /// Time during which all recorded entry versions are stored in the database. Entry versions that are older than
    /// `retain_detailed_history_for` will eventually be deleted, except for entry versions that are part of a snapshot.
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

/// Server state.
#[derive(Debug, Clone)]
pub struct Context {
    db_pool: PgPool,
    storage: Arc<Storage>,
    sources: Arc<Mutex<CachedSources>>,
    config: Config,
}

/// List of configured sources (clients).
#[derive(Debug)]
struct CachedSources {
    access_token_to_source_id: HashMap<String, SourceId>,
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

pub async fn run(
    config: Config,
    mut test_snapshot_tick_receiver: Option<mpsc::Receiver<()>>,
) -> Result<()> {
    info!("Connecting to database...");
    let db_pool = PgPool::connect(&config.database_url).await?;
    info!("Connected to database.");
    let ctx = Context {
        config: config.clone(),
        storage: Arc::new(Storage::new(config.storage_path)?),
        sources: Arc::new(Mutex::new(CachedSources {
            access_token_to_source_id: load_sources(&db_pool).await?,
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
            if let Some(test_snapshot_tick_receiver) = &mut test_snapshot_tick_receiver {
                test_snapshot_tick_receiver.recv().await;
            } else {
                interval.tick().await;
            }
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
                Err(error) => warn!(?error, "failed to accept"),
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
        // Content file upload and download.
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

/// Run a non-streaming request handler `f` and convert the result into a response.
async fn wrap_request<T, F, Fut>(
    ctx: handler::Context,
    request: Request<body::Incoming>,
    f: F,
) -> Result<Response<BoxBody<Bytes, Infallible>>, StatusCode>
where
    T: RequestToResponse + DeserializeOwned,
    T::Response: Serialize,
    F: FnOnce(handler::Context, T) -> Fut,
    Fut: Future<Output = Result<T::Response>>,
{
    let request = parse_request(request).await?;
    let response = f(ctx, request).await;
    Ok(Response::new(BodyExt::boxed(Full::new(
        serialize_response(response).map_err(|error| {
            warn!(?error, "failed to serialize response");
            StatusCode::INTERNAL_SERVER_ERROR
        })?,
    ))))
}

const ITEMS_PER_CHUNK: usize = 1024;

/// Serialize a streaming response item and send it to the stream.
/// Ensures that the passed `data` is of the expected stream item type.
async fn serialize_and_send<T>(
    y: &mut Yielder<Bytes>,
    data: Result<Option<&[StreamingResponseItem<T>]>>,
) where
    T: RequestToStreamingResponse,
    StreamingResponseItem<T>: Serialize,
{
    match serialize_response_with_length(data) {
        Ok(v) => y.send(v).await,
        Err(error) => {
            let error_data: Result<Option<&[StreamingResponseItem<T>]>> = Err(error);
            match serialize_response_with_length(error_data) {
                Ok(v) => y.send(v).await,
                Err(error) => {
                    error!(?error, "failed to serialize error response");
                }
            }
        }
    }
}

/// Run a streaming request handler `f` and convert the result into a response.
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
        let mut buf = Vec::new();
        while let Some(item) = rx.recv().await {
            match item {
                Ok(item) => {
                    buf.push(item);
                    if buf.len() >= ITEMS_PER_CHUNK {
                        serialize_and_send::<T>(&mut y, Ok(Some(&buf))).await;
                        buf.clear();
                    }
                }
                Err(err) => {
                    serialize_and_send::<T>(&mut y, Err(err)).await;
                    return;
                }
            }
        }
        if !buf.is_empty() {
            serialize_and_send::<T>(&mut y, Ok(Some(&buf))).await;
        }
        serialize_and_send::<T>(&mut y, Ok(None)).await;
    });

    Ok(Response::new(BodyExt::boxed(StreamBody::new(
        body_stream.map(|bytes| Ok(Frame::data(bytes))),
    ))))
}

/// Fetch request body and deserialize it to `T`.
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
    encoding::deserialize(&bytes).map_err(|err| {
        warn!(?err, "failed to deserialize request body");
        StatusCode::BAD_REQUEST
    })
}

fn serialize_response<T: Serialize>(data: Result<T>) -> anyhow::Result<Bytes> {
    encoding::serialize(&data.map_err(|err| {
        warn!(?err, "handler error");
        format!("{err:?}")
    }))
    .context("bincode serialization failed")
    .map(Into::into)
}

/// Create a buffer the contains the serialized size (4 bytes LE) followed by the serialized data.
fn serialize_response_with_length<T: Serialize>(data: Result<T>) -> anyhow::Result<Bytes> {
    // Reserve empty space for length at the beginning to avoid reallocation later.
    let mut buf = BytesMut::zeroed(4);
    let len = encoding::serialize_into(
        (&mut buf).writer(),
        &data.map_err(|err| {
            warn!(?err, "handler error");
            format!("{err:?}")
        }),
    )?;
    // Write the actual length.
    buf[0..4].copy_from_slice(&len.try_into_type::<u32>()?.to_le_bytes());
    Ok(buf.freeze())
}

/// Verify access token provided by the client and return the corresponding `SourceId`.
async fn auth(ctx: &Context, request: &Request<body::Incoming>) -> Result<SourceId> {
    let auth = request
        .headers()
        .get(AUTHORIZATION)
        .context("missing authorization header")?
        .to_str()?;
    let access_token = auth
        .strip_prefix("Bearer ")
        .context("authorization header is not Bearer")?;
    let mut sources = ctx.sources.lock().await;
    if sources.updated_at.elapsed() > SOURCES_CACHE_INTERVAL {
        sources.access_token_to_source_id = load_sources(&ctx.db_pool).await?;
        sources.updated_at = Instant::now();
    }
    sources
        .access_token_to_source_id
        .get(access_token)
        .copied()
        .context("invalid bearer token")
}

#[cfg(target_os = "linux")]
fn default_config_dir() -> Result<PathBuf> {
    Ok("/etc".into())
}

// Windows: %APPDATA% (%USERPROFILE%\AppData\Roaming);
// macOS: $HOME/Library/Application Support
#[cfg(not(target_os = "linux"))]
fn default_config_dir() -> Result<PathBuf> {
    dirs::config_dir().ok_or_else(|| anyhow::anyhow!("failed to get config dir"))
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(default_config_dir()?.join("rammingen-server.conf"))
}
