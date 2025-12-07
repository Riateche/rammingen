use {
    super::{ok_or_retry, Client, RequestError, DEFAULT_TIMEOUT, RESPONSE_TIMEOUT},
    crate::{
        content::LocalFileEntry,
        crypto::{Cipher, DecryptingWriter},
    },
    anyhow::{ensure, format_err, Context, Error, Result},
    cadd::{ops::Cadd, prelude::IntoType},
    fs_err::File,
    rammingen_protocol::util::maybe_block_in_place,
    reqwest::{header::CONTENT_LENGTH, Response},
    std::{fmt::Debug, io::Write, path::Path},
    tokio::time::timeout,
    tracing::instrument,
};

#[instrument(skip_all, fields(?path, ?local_entry))]
pub async fn download_and_decrypt(
    client: &Client,
    local_entry: &LocalFileEntry,
    path: impl AsRef<Path> + Send + Sync + Debug,
    cipher: &Cipher,
) -> Result<()> {
    let (actual_encrypted_size, decryptor) = ok_or_retry(|| async {
        let mut encrypted_content = content(client, local_entry, cipher).await?;
        let file = File::create(path.as_ref()).map_err(RequestError::application)?;
        let mut decryptor = DecryptingWriter::new(cipher, file);
        let mut actual_encrypted_size = 0;

        while let Some(chunk) = timeout(DEFAULT_TIMEOUT, encrypted_content.chunk())
            .await
            .map_err(RequestError::transport)?
            .map_err(RequestError::transport)?
        {
            let chunk_len = chunk
                .len()
                .try_into_type::<u64>()
                .map_err(RequestError::application)?;
            actual_encrypted_size = actual_encrypted_size
                .cadd(chunk_len)
                .map_err(RequestError::application)?;
            maybe_block_in_place(|| decryptor.write_all(&chunk))
                .map_err(RequestError::application)?;
        }
        Ok((actual_encrypted_size, decryptor))
    })
    .await?;
    ensure!(
        actual_encrypted_size == local_entry.encrypted_size,
        "encrypted size mismatch; actual {}, expected {}",
        actual_encrypted_size,
        local_entry.encrypted_size,
    );
    let (file, actual_hash, actual_size) = maybe_block_in_place(|| decryptor.finish())?;
    file.sync_all()?;
    ensure!(
        actual_size == local_entry.original_size,
        "content size mismatch; actual {actual_size}, expected {}",
        local_entry.original_size,
    );
    ensure!(
        actual_hash == local_entry.hash,
        "content hash mismatch; actual {actual_hash}, expected {}",
        local_entry.hash,
    );
    Ok(())
}

async fn content(
    client: &Client,
    local_entry: &LocalFileEntry,
    cipher: &Cipher,
) -> Result<Response, RequestError> {
    let url = cipher
        .encrypt_content_hash(&local_entry.hash)
        .and_then(|encrypted_hash| client.content_url(&encrypted_hash))
        .map_err(RequestError::Application)?;
    let response = timeout(
        DEFAULT_TIMEOUT,
        client
            .reqwest
            .get(url)
            .bearer_auth(client.token.as_unmasked_str())
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
    if declared_encrypted_size != local_entry.encrypted_size {
        return Err(RequestError::Application(format_err!(
            "encrypted size mismatch; declared {declared_encrypted_size}, expected {}",
            local_entry.encrypted_size,
        )));
    }
    Ok(response)
}
