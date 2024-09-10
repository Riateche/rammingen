use aes_siv::AeadCore;
use aes_siv::{aead::OsRng, Aes256SivAead, Nonce};
use anyhow::Result;
use byteorder::{ByteOrder, WriteBytesExt, LE};
use deflate::write::DeflateEncoder;
use deflate::CompressionOptions;
use fs_err::File;
use generic_array::typenum::ToInt;
use inflate::InflateWriter;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::cmp::min;
use std::io::{self, Write};
use std::path::Path;
use tempfile::SpooledTempFile;

use rammingen_protocol::ContentHash;

use crate::{content::EncryptedFileHead, crypto::Cipher};

/// Max size of encrypted file content that will be stored in memory.
/// Files exceeding this limit will be stored as a temporary file on disk.
const MAX_IN_MEMORY: usize = 32 * 1024 * 1024;

/// Max length of a file chunk that will be encrypted at once.
const BLOCK_SIZE: usize = 1024 * 1024;

/// File type marker that is stored at the beginning of every encrypted file.
const MAGIC_NUMBER: u32 = 3137690536;

// It should be a constant, but it currently doesn't work.
fn nonce_size() -> usize {
    <Aes256SivAead as AeadCore>::NonceSize::to_int()
}

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
        self.hasher.update(&buf[..len]);
        self.size += len as u64;
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
        OsRng.fill_bytes(&mut nonce);

        let ciphertext = self
            .cipher
            .encrypt_bytes(&nonce, &self.buf[..input_len])
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "encryption failed"))?;
        let output_size = nonce.len() + ciphertext.len();

        self.output.write_u32::<LE>(output_size as u32)?;
        self.output.write_all(&nonce)?;
        self.output.write_all(&ciphertext)?;
        self.encrypted_size += 4 + output_size as u64;

        self.buf.drain(..input_len);

        Ok(())
    }

    fn finish(mut self) -> io::Result<(W, u64)> {
        self.write_block()?;
        self.output.flush()?;
        Ok((self.output, self.encrypted_size))
    }
}

impl<'a, W: Write> Write for EncryptingWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        if self.buf.len() >= BLOCK_SIZE {
            self.write_block()?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

// Decrypts encrypted files.
pub struct DecryptingWriter<'a, W: Write> {
    // Whether the magic number has been read.
    got_header: bool,
    // Input data that is not yet decrypted.
    buf: Vec<u8>,
    cipher: &'a Cipher,
    output: InflateWriter<HashingWriter<W>>,
}

impl<'a, W: Write> DecryptingWriter<'a, W> {
    pub fn new(cipher: &'a Cipher, output: W) -> Self {
        Self {
            got_header: false,
            buf: Vec::new(),
            cipher,
            output: InflateWriter::new(HashingWriter::new(output)),
        }
    }

    pub fn finish(mut self) -> io::Result<(W, ContentHash, u64)> {
        self.process_block()?;
        if !self.buf.is_empty() {
            return Err(io::Error::new(io::ErrorKind::Other, "trailing data found"));
        }
        self.output.finish()?.finish()
    }

    fn process_block(&mut self) -> io::Result<()> {
        if !self.got_header {
            if self.buf.len() < 4 {
                return Ok(());
            }
            if LE::read_u32(&self.buf) != MAGIC_NUMBER {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "magic number mismatch",
                ));
            }
            self.buf.drain(..4);
            self.got_header = true;
        }
        if self.buf.len() < 4 {
            return Ok(());
        }
        let len: usize = LE::read_u32(&self.buf)
            .try_into()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let nonce_size = nonce_size();
        let max_block_size = BLOCK_SIZE + nonce_size + 16;
        if len > max_block_size {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("block size is too large (expected {max_block_size}, got {len})"),
            ));
        }
        let rest_of_data = &self.buf[4..];
        if rest_of_data.len() < len {
            return Ok(());
        }
        let chunk_data = &rest_of_data[..len];

        let nonce = chunk_data
            .get(..nonce_size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "chunk data is too short"))?;
        let nonce = Nonce::from_slice(nonce);
        let plaintext = self
            .cipher
            .decrypt_bytes(nonce, &chunk_data[nonce_size..])
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "decryption failed"))?;
        self.output.write_all(&plaintext)?;
        self.buf.drain(..4 + len);
        Ok(())
    }
}

impl<'a, W: Write> Write for DecryptingWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        self.process_block()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()?;
        Ok(())
    }
}

impl Cipher {
    pub fn encrypt_file(&self, path: impl AsRef<Path>) -> Result<EncryptedFileHead> {
        let mut input_file = File::open(path.as_ref())?;
        let output = SpooledTempFile::new(MAX_IN_MEMORY);
        let encryptor = EncryptingWriter::new(output, self)?;
        let encoder = DeflateEncoder::new(encryptor, CompressionOptions::high());
        let mut hasher = HashingWriter::new(encoder);
        io::copy(&mut input_file, &mut hasher)?;
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
}
