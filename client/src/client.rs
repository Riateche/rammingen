use aes_siv::Aes256SivAead;
use anyhow::{anyhow, bail, Result};
use byteorder::{ByteOrder, LE};
use derivative::Derivative;
use fs_err::File;
use futures::{Stream, StreamExt};
use reqwest::{header::CONTENT_LENGTH, Body, Method};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{self, Seek, SeekFrom, Write},
    path::Path,
    time::Duration,
};
use stream_generator::generate_try_stream;
use tempfile::SpooledTempFile;
use tokio::task::block_in_place;

use rammingen_protocol::{
    util::stream_file, ContentHash, RequestToResponse, RequestToStreamingResponse,
};

use crate::encryption::Decryptor;

#[derive(Derivative, Clone)]
pub struct Client {
    reqwest: reqwest::Client,
    server_url: String,
    #[derivative(Debug = "ignore")]
    token: String,
}

impl Client {
    pub fn new(server_url: &str, token: &str) -> Self {
        Self {
            server_url: server_url.into(),
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
            .request(Method::POST, format!("{}{}", self.server_url, R::NAME))
            .bearer_auth(&self.token)
            .body(bincode::serialize(&request)?)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        bincode::deserialize::<Result<R::Response, String>>(&response)?.map_err(anyhow::Error::msg)
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
                .request(Method::POST, format!("{}{}", this.server_url, R::NAME))
                .bearer_auth(&this.token)
                .body(request?)
                .send()
                .await?
                .error_for_status()?;
            let mut buf = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                buf.extend_from_slice(&chunk);
                while let Some((chunk, index)) = take_chunk(&buf) {
                    //crate::term::debug(format!("chunk from server: {:?}", chunk));
                    let data =
                        bincode::deserialize::<Result<Option<R::ResponseItem>, String>>(chunk)?
                            .map_err(anyhow::Error::msg)?;
                    buf.drain(..index);
                    if let Some(data) = data {
                        y.send(Ok(data)).await;
                    } else {
                        return Ok(());
                    }
                }
            }
            bail!("unexpected end of response");
        })
        .boxed()
    }

    pub async fn upload(&self, hash: &ContentHash, mut file: SpooledTempFile) -> Result<()> {
        let size = file.seek(SeekFrom::End(0))?;
        file.rewind()?;
        self.reqwest
            .put(format!("{}content/{}", self.server_url, hash))
            .bearer_auth(&self.token)
            .header(CONTENT_LENGTH, size)
            .body(Body::wrap_stream(stream_file(file).map(io::Result::Ok)))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn download(
        &self,
        hash: &ContentHash,
        path: impl AsRef<Path>,
        cipher: &Aes256SivAead,
    ) -> Result<()> {
        let mut response = self
            .reqwest
            .get(format!("{}content/{}", self.server_url, hash))
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

        let file = File::create(path.as_ref())?;
        let mut decryptor = Decryptor::new(cipher, file);
        let mut actual_len = 0;

        while let Some(chunk) = response.chunk().await? {
            actual_len += chunk.len() as u64;
            block_in_place(|| decryptor.write_all(&chunk))?;
        }
        block_in_place(|| decryptor.finish())?;
        if actual_len != header_len {
            bail!("content length mismatch");
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
