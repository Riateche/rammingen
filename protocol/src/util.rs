use std::{
    borrow::Cow,
    future::Future,
    io::{stdout, ErrorKind, Read, Write},
    path::{self, Path, MAIN_SEPARATOR, MAIN_SEPARATOR_STR},
    sync::Arc,
};

use anyhow::{anyhow, bail, Result};
use bytes::Bytes;
use fs_err::OpenOptions;
use futures::future;
use itertools::Itertools;
use tokio::{
    pin, select,
    sync::{mpsc, Mutex},
    task::block_in_place,
};
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tracing::warn;

const CONTENT_CHUNK_LEN: usize = 1024;

pub fn stream_file(file: Arc<Mutex<impl Read + Send + 'static>>) -> impl Stream<Item = Bytes> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(async move {
        let mut file = file.lock().await;
        let mut buf = vec![0u8; CONTENT_CHUNK_LEN];
        loop {
            match block_in_place(|| file.read(&mut buf)) {
                Ok(len) => {
                    if len == 0 {
                        break; // end of file
                    } else {
                        if tx.send(Bytes::copy_from_slice(&buf[0..len])).await.is_err() {
                            break; // receiver closed
                        }
                    }
                }
                Err(err) => {
                    warn!(?err, "failed to read content file");
                    break;
                }
            }
        }
    });
    ReceiverStream::new(rx)
}

pub fn try_exists(path: impl AsRef<Path>) -> Result<bool> {
    match fs_err::metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn archive_to_native_relative_path(relative_archive_path: &str) -> Cow<'_, str> {
    if MAIN_SEPARATOR == '/' {
        Cow::Borrowed(relative_archive_path)
    } else {
        Cow::Owned(relative_archive_path.split('/').join(MAIN_SEPARATOR_STR))
    }
}

pub fn native_to_archive_relative_path(relative_path: &Path) -> Result<String> {
    let mut result = Vec::new();
    for component in relative_path.components() {
        if let path::Component::Normal(component) = component {
            result.push(
                component
                    .to_str()
                    .ok_or_else(|| anyhow!("unsupported path: {:?}", relative_path))?,
            );
        } else {
            bail!("found invalid component in {:?}", relative_path);
        }
    }
    Ok(result.join("/"))
}

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
    pub async fn notify(&self, err: impl Into<anyhow::Error>) {
        let _ = self.0.send(err.into()).await;
    }

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
