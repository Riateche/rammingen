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
    time::Duration,
};
use stream_generator::generate_try_stream;
use tokio::task::block_in_place;

use rammingen_protocol::{
    endpoints::{RequestToResponse, RequestToStreamingResponse},
    util::stream_file,
    EncryptedContentHash, FileContent,
};

use crate::encryption::{encrypt_content_hash, Decryptor};

#[derive(Derivative, Clone)]
pub struct Client {
    reqwest: reqwest::Client,
    server_url: Url,
    #[derivative(Debug = "ignore")]
    token: String,
}

impl Client {
    pub fn new(server_url: Url, token: &str) -> Self {
        Self {
            server_url,
            token: token.into(),
            reqwest: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }

    pub async fn request<R>(&self, request: &R) -> Result<R::Response>
    where
        R: RequestToResponse + Serialize,
        R::Response: DeserializeOwned,
    {
        let response = self
            .reqwest
            .request(Method::POST, self.server_url.join(R::PATH)?)
            .bearer_auth(&self.token)
            .body(bincode::serialize(&request)?)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        bincode::deserialize::<Result<R::Response, String>>(&response)?
            .map_err(|msg| anyhow!("server error: {msg}"))
    }

    pub fn stream<R>(&self, request: &R) -> impl Stream<Item = Result<R::ResponseItem>>
    where
        R: RequestToStreamingResponse + Serialize + Send + Sync + 'static,
        R::ResponseItem: DeserializeOwned + Send + Sync + 'static,
    {
        let this = self.clone();
        let request = bincode::serialize(&request);
        generate_try_stream(|mut y| async move {
            let mut response = this
                .reqwest
                .request(Method::POST, this.server_url.join(R::PATH)?)
                .bearer_auth(&this.token)
                .body(request?)
                .send()
                .await?
                .error_for_status()?;
            let mut buf = Vec::new();
            while let Some(chunk) = response.chunk().await? {
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
        encrypted_file.rewind()?;
        self.reqwest
            .put(format!("{}content/{}", self.server_url, hash.to_url_safe()))
            .bearer_auth(&self.token)
            .header(CONTENT_LENGTH, size)
            .body(Body::wrap_stream(
                stream_file(encrypted_file).map(io::Result::Ok),
            ))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn download_and_decrypt(
        &self,
        content: &FileContent,
        path: impl AsRef<Path>,
        cipher: &Aes256SivAead,
    ) -> Result<()> {
        let encrypted_hash = encrypt_content_hash(&content.hash, cipher)?;
        let mut response = self
            .reqwest
            .get(format!(
                "{}content/{}",
                self.server_url,
                encrypted_hash.to_url_safe()
            ))
            .bearer_auth(&self.token)
            .send()
            .await?
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

        while let Some(chunk) = response.chunk().await? {
            actual_encrypted_size += chunk.len() as u64;
            block_in_place(|| decryptor.write_all(&chunk))?;
        }
        let (_, actual_hash, actual_original_size) = block_in_place(|| decryptor.finish())?;
        if actual_encrypted_size != header_len {
            bail!("content length mismatch");
        }
        if content.hash != actual_hash {
            bail!("content hash mismatch");
        }
        if content.original_size != actual_original_size {
            bail!("original size mismatch");
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
