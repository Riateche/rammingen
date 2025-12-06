use {
    anyhow::{bail, Context as _, Result},
    bytes::Bytes,
    fs_err::OpenOptions,
    futures::future,
    itertools::Itertools,
    std::{
        borrow::Cow,
        future::Future,
        io::{stdout, Read, Write},
        path::{self, Path, MAIN_SEPARATOR, MAIN_SEPARATOR_STR},
        sync::Arc,
    },
    tokio::{
        pin, select,
        sync::{mpsc, Mutex},
        task::block_in_place,
    },
    tokio_stream::{wrappers::ReceiverStream, Stream},
    tracing::warn,
};

const CONTENT_CHUNK_LEN: usize = 1024;

/// Create a stream that yields the content of `file`.
#[inline]
pub fn stream_file(file: Arc<Mutex<impl Read + Send + 'static>>) -> impl Stream<Item = Bytes> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(async move {
        let mut file = file.lock().await;
        let mut buf = vec![0u8; CONTENT_CHUNK_LEN];
        loop {
            match maybe_block_in_place(|| file.read(&mut buf)) {
                Ok(len) => {
                    if len == 0 {
                        break; // end of file
                    } else {
                        let Some(buf_data) = buf.get(..len) else {
                            warn!(?len, "file read returned invalid length");
                            break;
                        };
                        if tx.send(Bytes::copy_from_slice(buf_data)).await.is_err() {
                            break; // receiver closed
                        }
                    }
                }
                Err(error) => {
                    warn!(?error, "failed to read content file");
                    break;
                }
            }
        }
    });
    ReceiverStream::new(rx)
}

/// Convert a relative archive path (that always uses `/` as separator)
/// to a relative path with native separator for the current OS.
#[must_use]
#[inline]
pub fn archive_to_native_relative_path(relative_archive_path: &str) -> Cow<'_, str> {
    if MAIN_SEPARATOR == '/' {
        Cow::Borrowed(relative_archive_path)
    } else {
        Cow::Owned(relative_archive_path.split('/').join(MAIN_SEPARATOR_STR))
    }
}

/// Convert relative path with native separator for the current OS
/// to a relative archive path (that always uses `/` as separator).
///
/// `relative_path` should not contain `.` or `..`.
#[inline]
pub fn native_to_archive_relative_path(relative_path: &Path) -> Result<String> {
    let mut result = Vec::new();
    for component in relative_path.components() {
        if let path::Component::Normal(component) = component {
            result.push(
                component
                    .to_str()
                    .with_context(|| format!("unsupported path: {:?}", relative_path))?,
            );
        } else {
            bail!("found invalid component in {:?}", relative_path);
        }
    }
    Ok(result.join("/"))
}

/// Create a log writer that logs to the specified `log_file`, or to stdout if `log_file` is `None`.
#[inline]
pub fn log_writer(log_file: Option<&Path>) -> Result<Box<dyn Write + Send + Sync>> {
    if let Some(log_file) = log_file {
        Ok(Box::new(
            OpenOptions::new()
                .write(true)
                .append(true)
                .create(true)
                .open(log_file)?,
        ))
    } else {
        Ok(Box::new(stdout()))
    }
}

#[derive(Clone)]
pub struct ErrorSender(mpsc::Sender<anyhow::Error>);

impl ErrorSender {
    #[inline]
    pub async fn notify(&self, err: impl Into<anyhow::Error>) {
        let _ = self.0.send(err.into()).await;
    }

    #[inline]
    pub async fn unwrap_or_notify<T, E>(&self, value: Result<T, E>) -> T
    where
        E: Into<anyhow::Error>,
    {
        match value {
            Ok(value) => value,
            Err(err) => {
                self.notify(err).await;
                future::pending().await
            }
        }
    }
}

#[inline]
pub async fn interrupt_on_error<F, R, Fut>(f: F) -> Result<R>
where
    F: FnOnce(ErrorSender) -> Fut,
    Fut: Future<Output = Result<R>>,
{
    let (sender, mut receiver) = mpsc::channel(10);
    let fut = f(ErrorSender(sender));
    pin!(fut);
    select! {
        err = receiver.recv() => if let Some(err) = err {
            Err(err)
        } else {
            fut.await
        },
        r = &mut fut => r,
    }
}

/// Call `f` through `tokio::task::block_in_place` if possible.
#[inline]
pub fn maybe_block_in_place<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    if cfg!(target_os = "android") {
        // We use single-threaded executor on Android, so we have to execute it without `block_in_place`.
        f()
    } else {
        block_in_place(f)
    }
}
