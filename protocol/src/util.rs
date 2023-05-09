use std::{
    borrow::Cow,
    io::{stdout, ErrorKind, Read, Write},
    path::{self, Path, MAIN_SEPARATOR, MAIN_SEPARATOR_STR},
};

use anyhow::{anyhow, bail, Result};
use bytes::Bytes;
use chrono::{DateTime, FixedOffset, Utc};
use fs_err::OpenOptions;
use itertools::Itertools;
use tokio::{sync::mpsc, task::block_in_place};
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tracing::warn;

const CONTENT_CHUNK_LEN: usize = 1024;

pub fn stream_file(mut file: impl Read + Send + 'static) -> impl Stream<Item = Bytes> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(async move {
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

pub fn local_time(time: DateTime<Utc>) -> DateTime<FixedOffset> {
    time.into()
}
