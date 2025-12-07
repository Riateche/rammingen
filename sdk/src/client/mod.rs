mod download;

use {
    crate::{client::download::download_and_decrypt, content::LocalFileEntry, crypto::Cipher},
    anyhow::{Error, Result, bail, format_err},
    byteorder::{ByteOrder, LE},
    cadd::{ops::Cadd, prelude::IntoType},
    futures::{Stream, StreamExt},
    rammingen_protocol::{
        AccessToken, EncryptedContentHash,
        encoding::{self, deserialize},
        endpoints::{RequestToResponse, RequestToStreamingResponse},
        util::stream_file,
    },
    reqwest::{Body, Method, Url, header::CONTENT_LENGTH},
    serde::{Serialize, de::DeserializeOwned},
    std::{
        fmt::Debug,
        future::Future,
        io::{self, Read, Seek, SeekFrom},
        path::Path,
        sync::Arc,
        time::Duration,
    },
    stream_generator::generate_try_stream,
    tokio::{
        sync::Mutex,
        time::{sleep, timeout},
    },
    tracing::{instrument, warn},
};

/// Reuse created client or clone it in order to reuse a connection pool.
#[derive(Clone)]
pub struct Client {
    reqwest: reqwest::Client,
    server_url: Url,
    token: AccessToken,
}

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Loading large files may take a long time.
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(3600 * 24);

#[must_use]
#[inline]
pub fn upload_timeout(upload_size: u64) -> Duration {
    // Assuming upload speed above 1 MB/s.
    DEFAULT_TIMEOUT.saturating_add(Duration::from_micros(upload_size))
}

impl Client {
    #[inline]
    pub fn new(server_url: Url, token: AccessToken) -> anyhow::Result<Self> {
        Ok(Self {
            server_url,
            token,
            reqwest: reqwest::Client::builder()
                .timeout(DEFAULT_TIMEOUT)
                .build()?,
        })
    }

    #[instrument(skip_all)]
    #[inline(never)]
    pub async fn request_with_timeout<R>(
        &self,
        request: &R,
        timeout: Option<Duration>,
    ) -> Result<R::Response>
    where
        R: RequestToResponse + Serialize + Send + Sync,
        R::Response: DeserializeOwned + Send + Sync,
    {
        let url = self.server_url.join(R::PATH)?;
        let body = encoding::serialize(&request)?;
        let bytes = ok_or_retry(|| async {
            let mut post_request = self
                .reqwest
                .request(Method::POST, url.clone())
                .bearer_auth(self.token.as_unmasked_str())
                .body(body.clone());
            if let Some(timeout) = timeout {
                post_request = post_request.timeout(timeout);
            }

            post_request
                .send()
                .await
                .map_err(RequestError::transport)?
                .error_for_status()
                .map_err(RequestError::application)?
                .bytes()
                .await
                .map_err(RequestError::transport)
        })
        .await?;
        deserialize::<Result<_, String>>(&bytes)?.map_err(|msg| format_err!("server error: {msg}"))
    }

    #[inline]
    pub async fn request<R>(&self, request: &R) -> Result<R::Response>
    where
        R: RequestToResponse + Serialize + Send + Sync,
        R::Response: DeserializeOwned + Send + Sync,
    {
        self.request_with_timeout(request, None).await
    }

    #[inline(never)]
    pub fn stream<R>(&self, request: &R) -> impl Stream<Item = Result<R::ResponseItem>> + use<R>
    where
        R: RequestToStreamingResponse + Serialize + Send + Sync + 'static,
        R::ResponseItem: DeserializeOwned + Send + Sync + 'static,
    {
        let this = self.clone();
        let request = encoding::serialize(&request);
        generate_try_stream(|mut y| async move {
            let mut response = timeout(
                DEFAULT_TIMEOUT,
                this.reqwest
                    .request(Method::POST, this.server_url.join(R::PATH)?)
                    .timeout(RESPONSE_TIMEOUT)
                    .bearer_auth(this.token.as_unmasked_str())
                    .body(request?)
                    .send(),
            )
            .await??
            .error_for_status()?;
            let mut buf = Vec::new();
            while let Some(chunk) = timeout(DEFAULT_TIMEOUT, response.chunk()).await?? {
                buf.extend_from_slice(&chunk);
                while let Some((bytes, index)) = take_chunk(&buf)? {
                    let data = deserialize::<Result<Option<Vec<R::ResponseItem>>, String>>(bytes)?
                        .map_err(|msg| format_err!("server error: {msg}"))?;

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

    #[instrument(skip_all, fields(?hash))]
    #[inline(never)]
    pub async fn upload(
        &self,
        hash: &EncryptedContentHash,
        mut encrypted_file: impl Read + Seek + Send + 'static,
    ) -> Result<()> {
        let size = encrypted_file.seek(SeekFrom::End(0))?;
        let encrypted_file = Arc::new(Mutex::new(encrypted_file));
        let url = self.content_url(hash)?;
        ok_or_retry(|| async {
            encrypted_file
                .lock()
                .await
                .rewind()
                .map_err(RequestError::application)?;
            self.reqwest
                .put(url.clone())
                .timeout(upload_timeout(size))
                .bearer_auth(self.token.as_unmasked_str())
                .header(CONTENT_LENGTH, size)
                .body(Body::wrap_stream(
                    stream_file(Arc::clone(&encrypted_file)).map(io::Result::Ok),
                ))
                .send()
                .await
                .map_err(RequestError::transport)?
                .error_for_status()
                .map_err(RequestError::application)?;
            Ok(())
        })
        .await
    }

    fn content_url(&self, hash: &EncryptedContentHash) -> Result<Url> {
        let mut url = self.server_url.clone();
        url.path_segments_mut()
            .map_err(|()| format_err!("failed server URL extension"))?
            .push("content")
            .push(&hash.to_url_safe());
        Ok(url)
    }

    #[inline]
    pub async fn download_and_decrypt(
        &self,
        local_entry: &LocalFileEntry,
        path: impl AsRef<Path> + Send + Sync + Debug,
        cipher: &Cipher,
    ) -> Result<()> {
        download_and_decrypt(self, local_entry, path, cipher).await
    }
}

fn take_chunk(buf: &[u8]) -> Result<Option<(&[u8], usize)>> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = LE::read_u32(buf).try_into_type::<usize>()?;
    let end = len.cadd(4_usize)?;
    if let Some(bytes) = buf.get(4..end) {
        Ok(Some((bytes, end)))
    } else {
        Ok(None)
    }
}

/// Retries the request if an error arises due to the transport.
async fn ok_or_retry<T, F, Fut>(mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, RequestError>>,
{
    const NUM_RETRIES: usize = 5;
    const RETRY_PERIOD: Duration = Duration::from_secs(10);
    let mut attempt = 0;
    loop {
        attempt = attempt.cadd(1_usize)?;
        let transport_err = match f().await {
            Ok(x) => break Ok(x),
            Err(RequestError::Application(err)) => break Err(err),
            Err(RequestError::Transport(err)) => err,
        };
        if attempt >= NUM_RETRIES {
            break Err(transport_err);
        }
        warn!(error = %transport_err, attempt, "transport failed, will retry");
        sleep(RETRY_PERIOD).await;
    }
}

enum RequestError {
    Transport(Error),
    Application(Error),
}

impl RequestError {
    fn application(err: impl Into<Error>) -> Self {
        Self::Application(err.into())
    }

    fn transport(err: impl Into<Error>) -> Self {
        Self::Transport(err.into())
    }
}

impl From<RequestError> for Error {
    #[inline]
    fn from(err: RequestError) -> Self {
        match err {
            RequestError::Transport(err) | RequestError::Application(err) => err,
        }
    }
}
