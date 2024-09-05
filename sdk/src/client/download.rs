use std::{io::Write, path::Path, time::Duration};

use anyhow::{anyhow, bail, Result};
use fs_err::File;
use reqwest::header::CONTENT_LENGTH;
use tokio::{
    task::block_in_place,
    time::{sleep, timeout},
};
use tracing::warn;

use super::{Client, DEFAULT_TIMEOUT, NUM_RETRIES, RETRY_INTERVAL};
use crate::{
    content::DecryptedFileContent,
    crypto::{Cipher, DecryptingWriter},
};

impl Client {
    pub async fn download_and_decrypt(
        &self,
        content: &DecryptedFileContent,
        path: impl AsRef<Path>,
        cipher: &Cipher,
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
        cipher: &Cipher,
    ) -> Result<()> {
        let encrypted_hash = cipher.encrypt_content_hash(&content.hash)?;
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
        let mut decryptor = DecryptingWriter::new(cipher, file);
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
