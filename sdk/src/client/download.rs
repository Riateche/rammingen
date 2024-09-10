use std::{fmt::Debug, io::Write, path::Path};

use anyhow::{ensure, format_err, Context, Error, Result};
use fs_err::File;
use reqwest::{header::CONTENT_LENGTH, Response};
use tokio::{task::block_in_place, time::timeout};
use tracing::instrument;

use super::{ok_or_retry, Client, RequestError, DEFAULT_TIMEOUT, RESPONSE_TIMEOUT};
use crate::{
    content::DecryptedContentHead,
    crypto::{Cipher, DecryptingWriter},
};

impl Client {
    #[instrument(skip_all, fields(?path, ?handle))]
    pub async fn download_and_decrypt(
        &self,
        handle: &DecryptedContentHead,
        path: impl AsRef<Path> + Debug,
        cipher: &Cipher,
    ) -> Result<()> {
        let (actual_encrypted_size, decryptor) = ok_or_retry(|| async {
            let mut encrypted_content = self.content(handle, cipher).await?;
            let file = File::create(path.as_ref()).map_err(RequestError::application)?;
            let mut decryptor = DecryptingWriter::new(cipher, file);
            let mut actual_encrypted_size = 0;

            while let Some(chunk) = timeout(DEFAULT_TIMEOUT, encrypted_content.chunk())
                .await
                .map_err(RequestError::transport)?
                .map_err(RequestError::transport)?
            {
                actual_encrypted_size += chunk.len() as u64;
                block_in_place(|| decryptor.write_all(&chunk))
                    .map_err(RequestError::application)?;
            }
            Ok((actual_encrypted_size, decryptor))
        })
        .await?;
        ensure!(
            actual_encrypted_size == handle.encrypted_size,
            "encrypted size mismatch; actual {}, expected {}",
            actual_encrypted_size,
            handle.encrypted_size,
        );
        let (_, actual_hash, actual_size) = block_in_place(|| decryptor.finish())?;
        ensure!(
            actual_size == handle.original_size,
            "content size mismatch; actual {actual_size}, expected {}",
            handle.original_size,
        );
        ensure!(
            actual_hash == handle.hash,
            "content hash mismatch; actual {actual_hash}, expected {}",
            handle.hash,
        );
        Ok(())
    }

    async fn content(
        &self,
        handle: &DecryptedContentHead,
        cipher: &Cipher,
    ) -> Result<Response, RequestError> {
        let url = cipher
            .encrypt_content_hash(&handle.hash)
            .and_then(|encrypted_hash| self.content_url(&encrypted_hash))
            .map_err(RequestError::Application)?;
        let response = timeout(
            DEFAULT_TIMEOUT,
            self.reqwest
                .get(url)
                .bearer_auth(self.token.as_unmasked_str())
                .timeout(RESPONSE_TIMEOUT)
                .send(),
        )
        .await
        .map_err(RequestError::transport)?
        .map_err(RequestError::transport)?
        .error_for_status()
        .map_err(RequestError::application)?;
        let declared_encrypted_size: u64 = response
            .headers()
            .get(CONTENT_LENGTH)
            .context("missing content length header")
            .and_then(|len| {
                len.to_str()
                    .map_err(Error::from)
                    .and_then(|len| Ok(len.parse()?))
                    .context("failed content length parsing")
            })
            .map_err(RequestError::Application)?;
        if declared_encrypted_size != handle.encrypted_size {
            return Err(RequestError::Application(format_err!(
                "encrypted size mismatch; declared {declared_encrypted_size}, expected {}",
                handle.encrypted_size,
            )));
        }
        Ok(response)
    }
}
