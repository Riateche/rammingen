use {
    crate::{content::EncryptedFileHead, crypto::Cipher},
    aes_siv::{AeadCore, Aes256SivAead, Nonce},
    anyhow::Result,
    byteorder::{ByteOrder, WriteBytesExt, LE},
    cadd::{ops::Cadd, prelude::IntoType},
    deflate::{write::DeflateEncoder, CompressionOptions},
    generic_array::typenum::ToInt,
    inflate::InflateWriter,
    rammingen_protocol::ContentHash,
    rand::{rngs::OsRng, TryRngCore},
    sha2::{Digest, Sha256},
    std::{
        cmp::min,
        io::{self, Read, Write},
    },
    tempfile::SpooledTempFile,
    tracing::error,
};

/// Max size of encrypted file content that will be stored in memory.
/// Files exceeding this limit will be stored as a temporary file on disk.
const MAX_IN_MEMORY: usize = 32 * 1024 * 1024;

/// Max length of an unencoded file chunk that will be encrypted at once.
const BLOCK_SIZE: usize = 1024 * 1024;

/// Size of the nonce included in each chunk.
const NONCE_SIZE: usize = <Aes256SivAead as AeadCore>::NonceSize::INT;

/// Max size of the encoded block payload (nonce + encrypted file data)
const MAX_ENCODED_BLOCK_SIZE: usize = BLOCK_SIZE + NONCE_SIZE + 16;

/// File type marker that is stored at the beginning of every encrypted file.
const MAGIC_NUMBER: u32 = 3137690536;

/// Passes through any writes and calculates Sha256 hash and size of the written data.
struct HashingWriter<W> {
    hasher: Sha256,
    size: u64,
    inner: W,
}

impl<W> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            hasher: Sha256::new(),
            inner,
            size: 0,
        }
    }

    pub fn finish(mut self) -> io::Result<(W, ContentHash, u64)>
    where
        W: Write,
    {
        self.inner.flush()?;
        let hash = ContentHash::new(self.hasher.finalize().into());
        Ok((self.inner, hash, self.size))
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = self.inner.write(buf)?;
        let written = buf
            .get(..len)
            .ok_or_else(|| io::Error::other("inner writer returned invalid length"))?;
        self.hasher.update(written);
        self.size = self
            .size
            .cadd(len.try_into_type::<u64>().map_err(io::Error::other)?)
            .map_err(io::Error::other)?;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Writes encrypted blocks of file content.
struct EncryptingWriter<'a, W> {
    // Input data of the currently accumulated block.
    buf: Vec<u8>,
    output: W,
    cipher: &'a Cipher,
    encrypted_size: u64,
}

impl<'a, W: Write> EncryptingWriter<'a, W> {
    fn new(mut output: W, cipher: &'a Cipher) -> io::Result<Self> {
        output.write_u32::<LE>(MAGIC_NUMBER)?;
        Ok(Self {
            buf: Vec::new(),
            output,
            cipher,
            // size of magic number
            encrypted_size: 4,
        })
    }

    fn write_block(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let input_len = min(self.buf.len(), BLOCK_SIZE);
        let mut nonce = Nonce::default();
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|err| io::Error::other(format!("OsRng error: {err:?}")))?;

        #[expect(
            clippy::indexing_slicing,
            reason = "input_len < self.buf.len() - checked above"
        )]
        let ciphertext = self
            .cipher
            .encrypt_bytes(&nonce, &self.buf[..input_len])
            .map_err(|_e| io::Error::other("encryption failed"))?;
        let output_size = nonce
            .len()
            .cadd(ciphertext.len())
            .map_err(io::Error::other)?;

        self.output
            .write_u32::<LE>(output_size.try_into().map_err(io::Error::other)?)?;
        self.output.write_all(&nonce)?;
        self.output.write_all(&ciphertext)?;
        let written_size = output_size
            .try_into_type::<u64>()
            .map_err(io::Error::other)?
            .cadd(4_u64)
            .map_err(io::Error::other)?;
        self.encrypted_size = self
            .encrypted_size
            .cadd(written_size)
            .map_err(io::Error::other)?;

        self.buf.drain(..input_len);

        Ok(())
    }

    fn finish(mut self) -> io::Result<(W, u64)> {
        self.write_block()?;
        self.output.flush()?;
        Ok((self.output, self.encrypted_size))
    }
}

impl<W: Write> Write for EncryptingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        if self.buf.len() >= BLOCK_SIZE {
            self.write_block()?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

// Decrypts encrypted files.
pub struct DecryptingWriter<'a, W: Write> {
    // Whether the magic number has been read.
    got_file_header: bool,
    // Input data that is not yet decrypted.
    buf: Vec<u8>,
    cipher: &'a Cipher,
    output: InflateWriter<HashingWriter<W>>,
}

impl<'a, W: Write> DecryptingWriter<'a, W> {
    #[inline]
    pub fn new(cipher: &'a Cipher, output: W) -> Self {
        Self {
            got_file_header: false,
            buf: Vec::new(),
            cipher,
            output: InflateWriter::new(HashingWriter::new(output)),
        }
    }

    #[inline]
    pub fn finish(mut self) -> io::Result<(W, ContentHash, u64)> {
        self.process_block()?;
        if !self.buf.is_empty() {
            return Err(io::Error::other("trailing data found"));
        }
        self.output.finish()?.finish()
    }

    fn process_block(&mut self) -> io::Result<()> {
        if !self.got_file_header {
            if self.buf.len() < 4 {
                return Ok(());
            }
            if LE::read_u32(&self.buf) != MAGIC_NUMBER {
                return Err(io::Error::other("magic number mismatch"));
            }
            self.buf.drain(..4);
            self.got_file_header = true;
        }

        let Some((len_bytes, rest_of_data)) = self.buf.split_at_checked(4) else {
            // Haven't received enough data yet.
            return Ok(());
        };

        let len: usize = LE::read_u32(len_bytes)
            .try_into()
            .map_err(io::Error::other)?;
        if len > MAX_ENCODED_BLOCK_SIZE {
            return Err(io::Error::other(format!(
                "block size is too large (expected {MAX_ENCODED_BLOCK_SIZE}, got {len})"
            )));
        }

        let Some(chunk_data) = &rest_of_data.get(..len) else {
            // Haven't received enough data yet.
            return Ok(());
        };

        let (nonce_bytes, encrypted_bytes) = chunk_data
            .split_at_checked(NONCE_SIZE)
            .ok_or_else(|| io::Error::other("chunk data is too short"))?;
        let nonce = Nonce::try_from(nonce_bytes).map_err(io::Error::other)?;
        let plaintext = self
            .cipher
            .decrypt_bytes(&nonce, encrypted_bytes)
            .map_err(|err| {
                error!(?err, "decryption failed");
                io::Error::other("decryption failed")
            })?;
        self.output.write_all(&plaintext)?;
        let bytes_read = len.cadd(4_usize).map_err(io::Error::other)?;
        self.buf.drain(..bytes_read);
        Ok(())
    }
}

impl<W: Write> Write for DecryptingWriter<'_, W> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        self.process_block()?;
        Ok(buf.len())
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()?;
        Ok(())
    }
}

#[expect(clippy::shadow_unrelated, reason = "false positive")]
pub fn encrypt_file_content(
    cipher: &Cipher,
    mut file_content: impl Read,
) -> Result<EncryptedFileHead> {
    let output = SpooledTempFile::new(MAX_IN_MEMORY);
    let encryptor = EncryptingWriter::new(output, cipher)?;
    let encoder = DeflateEncoder::new(encryptor, CompressionOptions::high());
    let mut hasher = HashingWriter::new(encoder);
    io::copy(&mut file_content, &mut hasher)?;
    let (encoder, hash, original_size) = hasher.finish()?;
    let encryptor = encoder.finish()?;
    let (file, encrypted_size) = encryptor.finish()?;
    Ok(EncryptedFileHead {
        file,
        hash,
        original_size,
        encrypted_size,
    })
}
