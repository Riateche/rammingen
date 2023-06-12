use aes_siv::Aes256SivAead;
use anyhow::{anyhow, bail, Result};
use byteorder::{ByteOrder, LE};
use derivative::Derivative;
use fs_err::File;
use futures::{Stream, StreamExt};
use reqwest::{header::CONTENT_LENGTH, Body, Method, Url};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
    time::Duration,
};
use stream_generator::generate_try_stream;
use tokio::{
    sync::Mutex,
    task::block_in_place,
    time::{sleep, timeout},
};
use tracing::warn;

use rammingen_protocol::{
    endpoints::{RequestToResponse, RequestToStreamingResponse},
    util::stream_file,
    EncryptedContentHash,
};

use crate::{
    data::DecryptedFileContent,
    encryption::{encrypt_content_hash, Decryptor},
};

#[derive(Derivative, Clone)]
pub struct Client {
    reqwest: reqwest::Client,
    server_url: Url,
    #[derivative(Debug = "ignore")]
    token: String,
}

pub const NUM_RETRIES: usize = 5;
pub const RETRY_INTERVAL: Duration = Duration::from_secs(10);
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub fn upload_timeout(upload_size: u64) -> Duration {
    DEFAULT_TIMEOUT + Duration::from_micros(upload_size)
}

impl Client {
    pub fn new(server_url: Url, token: &str) -> Self {
        Self {
            server_url,
            token: token.into(),
            reqwest: reqwest::Client::builder()
                .timeout(DEFAULT_TIMEOUT)
                .build()
                .unwrap(),
        }
    }

    pub async fn request_with_timeout<R>(
        &self,
        request: &R,
        timeout: Option<Duration>,
    ) -> Result<R::Response>
    where
        R: RequestToResponse + Serialize,
        R::Response: DeserializeOwned,
    {
        let mut i = 0;
        loop {
            i += 1;

            let result = self.request_with_timeout_once(request, timeout).await;
            match result {
                Ok(r) => return Ok(r),
                Err(err) => {
                    if i == NUM_RETRIES {
                        return Err(err);
                    } else {
                        warn!(?err, "request failed, will retry");
                        sleep(RETRY_INTERVAL).await;
                    }
                }
            }
        }
    }

    async fn request_with_timeout_once<R>(
        &self,
        request: &R,
        timeout: Option<Duration>,
    ) -> Result<R::Response>
    where
        R: RequestToResponse + Serialize,
        R::Response: DeserializeOwned,
    {
        let mut request = self
            .reqwest
            .request(Method::POST, self.server_url.join(R::PATH)?)
            .bearer_auth(&self.token)
            .body(bincode::serialize(&request)?);
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }

        let response = request.send().await?.error_for_status()?.bytes().await?;

        bincode::deserialize::<Result<R::Response, String>>(&response)?
            .map_err(|msg| anyhow!("server error: {msg}"))
    }

    pub async fn request<R>(&self, request: &R) -> Result<R::Response>
    where
        R: RequestToResponse + Serialize,
        R::Response: DeserializeOwned,
    {
        self.request_with_timeout(request, None).await
    }

    pub fn stream<R>(&self, request: &R) -> impl Stream<Item = Result<R::ResponseItem>>
    where
        R: RequestToStreamingResponse + Serialize + Send + Sync + 'static,
        R::ResponseItem: DeserializeOwned + Send + Sync + 'static,
    {
        let this = self.clone();
        let request = bincode::serialize(&request);
        generate_try_stream(|mut y| async move {
            let mut response = timeout(
                DEFAULT_TIMEOUT,
                this.reqwest
                    .request(Method::POST, this.server_url.join(R::PATH)?)
                    .timeout(Duration::from_secs(3600 * 24))
                    .bearer_auth(&this.token)
                    .body(request?)
                    .send(),
            )
            .await??
            .error_for_status()?;
            let mut buf = Vec::new();
            while let Some(chunk) = timeout(DEFAULT_TIMEOUT, response.chunk()).await?? {
                buf.extend_from_slice(&chunk);
                while let Some((chunk, index)) = take_chunk(&buf) {
                    let data =
                        bincode::deserialize::<Result<Option<Vec<R::ResponseItem>>, String>>(
                            chunk,
                        )?
                        .map_err(|msg| anyhow!("server error: {msg}"))?;

                    buf.drain(..index);
                    if let Some(data) = data {
                        for item in data {
                            y.send(Ok(item)).await;
                        }
                    } else {
                        return Ok(());
                    }
                }
            }
            bail!("unexpected end of response");
        })
        .boxed()
    }

    pub async fn upload(
        &self,
        hash: &EncryptedContentHash,
        mut encrypted_file: impl Read + Seek + Send + 'static,
    ) -> Result<()> {
        let size = encrypted_file.seek(SeekFrom::End(0))?;
        let encrypted_file = Arc::new(Mutex::new(encrypted_file));
        let mut i = 0;
        loop {
            i += 1;
            encrypted_file.lock().await.rewind()?;
            let result = self
                .reqwest
                .put(format!("{}content/{}", self.server_url, hash.to_url_safe()))
                .timeout(upload_timeout(size))
                .bearer_auth(&self.token)
                .header(CONTENT_LENGTH, size)
                .body(Body::wrap_stream(
                    stream_file(encrypted_file.clone()).map(io::Result::Ok),
                ))
                .send()
                .await
                .and_then(|r| r.error_for_status());
            match result {
                Ok(_) => return Ok(()),
                Err(err) => {
                    if i == NUM_RETRIES {
                        return Err(err.into());
                    } else {
                        warn!(?err, "upload request failed, will retry");
                        sleep(RETRY_INTERVAL).await;
                    }
                }
            }
        }
    }

    pub async fn download_and_decrypt(
        &self,
        content: &DecryptedFileContent,
        path: impl AsRef<Path>,
        cipher: &Aes256SivAead,
    ) -> Result<()> {
        let mut i = 0;
        loop {
            i += 1;

            let result = self
                .download_and_decrypt_once(content, path.as_ref(), cipher)
                .await;
            match result {
                Ok(r) => return Ok(r),
                Err(err) => {
                    if i == NUM_RETRIES {
                        return Err(err);
                    } else {
                        warn!(?err, "request failed, will retry");
                        sleep(RETRY_INTERVAL).await;
                    }
                }
            }
        }
    }

    async fn download_and_decrypt_once(
        &self,
        content: &DecryptedFileContent,
        path: impl AsRef<Path>,
        cipher: &Aes256SivAead,
    ) -> Result<()> {
        let encrypted_hash = encrypt_content_hash(&content.hash, cipher)?;
        let mut response = timeout(
            DEFAULT_TIMEOUT,
            self.reqwest
                .get(format!(
                    "{}content/{}",
                    self.server_url,
                    encrypted_hash.to_url_safe()
                ))
                .bearer_auth(&self.token)
                .timeout(Duration::from_secs(3600 * 24))
                .send(),
        )
        .await??
        .error_for_status()?;

        let header_len: u64 = response
            .headers()
            .get(CONTENT_LENGTH)
            .ok_or_else(|| anyhow!("missing content length header"))?
            .to_str()?
            .parse()?;

        if content.encrypted_size != header_len {
            bail!("encrypted size mismatch");
        }

        let file = File::create(path.as_ref())?;
        let mut decryptor = Decryptor::new(cipher, file);
        let mut actual_encrypted_size = 0;

        while let Some(chunk) = timeout(DEFAULT_TIMEOUT, response.chunk()).await?? {
            actual_encrypted_size += chunk.len() as u64;
            block_in_place(|| decryptor.write_all(&chunk))?;
        }
        let (_, actual_hash, actual_original_size) = block_in_place(|| decryptor.finish())?;
        if actual_encrypted_size != header_len {
            bail!("content length mismatch");
        }
        if content.original_size != actual_original_size {
            bail!(
                "original size mismatch (expected {}, got {})",
                content.original_size,
                actual_original_size
            );
        }
        if content.hash != actual_hash {
            bail!("content hash mismatch");
        }
        Ok(())
    }
}

fn take_chunk(buf: &[u8]) -> Option<(&[u8], usize)> {
    if buf.len() < 4 {
        return None;
    }
    let len = LE::read_u32(buf) as usize;
    if buf.len() < 4 + len {
        return None;
    }
    Some((&buf[4..4 + len], 4 + len))
}
