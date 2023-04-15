use aes_siv::Aes256SivAead;
use anyhow::{anyhow, bail, Result};
use fs_err::File;
use futures::StreamExt;
use reqwest::{header::CONTENT_LENGTH, Body, Method};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::task::block_in_place;

use rammingen_protocol::{util::stream_file, ContentHash, RequestToResponse};

use crate::encryption::Decryptor;

#[derive(Debug, Clone)]
pub struct Client {
    reqwest: reqwest::Client,
    server_url: String,
}

impl Client {
    pub fn new(server_url: &str) -> Self {
        Self {
            server_url: server_url.into(),
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
        Ok(self
            .reqwest
            .request(Method::POST, format!("{}{}", self.server_url, R::NAME))
            .json(request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn upload(&self, hash: &ContentHash, file: File) -> Result<()> {
        self.reqwest
            .put(format!("{}content/{}", self.server_url, hash))
            .header(CONTENT_LENGTH, file.metadata()?.len())
            .body(Body::wrap_stream(stream_file(file).map(io::Result::Ok)))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn download(
        &self,
        hash: &ContentHash,
        path: PathBuf,
        cipher: Arc<Aes256SivAead>,
    ) -> Result<()> {
        let mut response = self
            .reqwest
            .get(format!("{}content/{}", self.server_url, hash))
            .send()
            .await?
            .error_for_status()?;
        let header_len: u64 = response
            .headers()
            .get(CONTENT_LENGTH)
            .ok_or_else(|| anyhow!("missing content length header"))?
            .to_str()?
            .parse()?;

        let file = File::open(path)?;
        let mut decryptor = Decryptor::new(&cipher, file);
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
