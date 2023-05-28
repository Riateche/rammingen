//! All encryption operations use AES-SIV.
//!
//! Data that will be stored in the server's database is encrypted using zero nonce,
//! so the result is deterministic. This is important, e.g., for paths, because we
//! should be able to encrypt the path again and retrieve it from the server.
//! For file content, a random nonce is used for each block.
//!
//! File hash and size are encrypted using a single pass of AES-SIV with a zero nonce.
//!
//! When encrypting an archive path, it's split into components, and each component
//! is encrypted individually using a single pass of AES-SIV with a zero nonce, and then
//! encoded in base64. An encrypted path is then reconstructed from the encrypted components.
//! Thus, encrypted path is still a valid archive path, and
//! parent-child relationships are preserved even in encrypted form. This is important for
//! certain server operations. For example, if a MovePath or RemovePath command is issued,
//! the server should be able to find all paths nested in the specified path.
//!
//! When encrypting file content, it's first compressed using deflate and then split into fixed-size blocks.
//! For each block, a random nonce is chosen. The nonce and encrypted block data are written to the encrypted file
//! in the following form:
//!
//! - block size (32 bits, little endian) - length of the following block (nonce + encrypted content)
//! - nonce (128 bits) - the random nonce used to encrypt this block
//! - encrypted content
//!
//! Integrity of the file content is ensured on decryption by checking the resulting file content hash.

use aes_siv::aead::Aead;
use aes_siv::AeadCore;
use aes_siv::{aead::OsRng, Aes256SivAead, Nonce};
use anyhow::{anyhow, bail, Result};
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use byteorder::{ByteOrder, WriteBytesExt, LE};
use deflate::write::DeflateEncoder;
use deflate::CompressionOptions;
use fs_err::File;
use inflate::InflateWriter;
use rammingen_protocol::{
    ArchivePath, ContentHash, EncryptedArchivePath, EncryptedContentHash, EncryptedSize,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::cmp::min;
use std::io::{self, Write};
use std::path::Path;
use tempfile::SpooledTempFile;
use typenum::ToInt;

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
        self.hasher.update(buf);
        self.size += buf.len() as u64;
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
    cipher: &'a Aes256SivAead,
    encrypted_size: u64,
}

impl<'a, W: Write> EncryptingWriter<'a, W> {
    fn new(mut output: W, cipher: &'a Aes256SivAead) -> io::Result<Self> {
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
            .encrypt(&nonce, &self.buf[..input_len])
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

pub struct EncryptedFileData {
    pub file: SpooledTempFile,
    pub hash: ContentHash,
    pub original_size: u64,
    pub encrypted_size: u64,
}

pub fn encrypt_file(path: impl AsRef<Path>, cipher: &Aes256SivAead) -> Result<EncryptedFileData> {
    let mut input_file = File::open(path.as_ref())?;
    let output = SpooledTempFile::new(MAX_IN_MEMORY);
    let encryptor = EncryptingWriter::new(output, cipher)?;
    let encoder = DeflateEncoder::new(encryptor, CompressionOptions::high());
    let mut hasher = HashingWriter::new(encoder);
    io::copy(&mut input_file, &mut hasher)?;
    let (encoder, hash, original_size) = hasher.finish()?;
    let encryptor = encoder.finish()?;
    let (file, encrypted_size) = encryptor.finish()?;
    Ok(EncryptedFileData {
        file,
        hash,
        original_size,
        encrypted_size,
    })
}

// Decrypts encrypted files.
pub struct Decryptor<'a, W: Write> {
    // Whether the magic number has been read.
    got_header: bool,
    // Input data that is not yet decrypted.
    buf: Vec<u8>,
    cipher: &'a Aes256SivAead,
    output: InflateWriter<HashingWriter<W>>,
}

impl<'a, W: Write> Decryptor<'a, W> {
    pub fn new(cipher: &'a Aes256SivAead, output: W) -> Self {
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
        if len > BLOCK_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "block size is too large",
            ));
        }
        let rest_of_data = &self.buf[4..];
        if rest_of_data.len() < len {
            return Ok(());
        }
        let chunk_data = &rest_of_data[..len];

        let nonce_size = nonce_size();
        let nonce = chunk_data
            .get(..nonce_size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "chunk data is too short"))?;
        let nonce = Nonce::from_slice(nonce);
        let plaintext = self
            .cipher
            .decrypt(nonce, &chunk_data[nonce_size..])
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "decryption failed"))?;
        self.output.write_all(&plaintext)?;
        self.buf.drain(..4 + len);
        Ok(())
    }
}

impl<'a, W: Write> Write for Decryptor<'a, W> {
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

pub fn encrypt_str(value: &str, cipher: &Aes256SivAead) -> Result<String> {
    let ciphertext = cipher
        .encrypt(&Nonce::default(), value.as_bytes())
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok(BASE64_URL_SAFE_NO_PAD.encode(ciphertext))
}

pub fn decrypt_str(value: &str, cipher: &Aes256SivAead) -> Result<String> {
    let ciphertext = BASE64_URL_SAFE_NO_PAD.decode(value)?;
    let plaintext = cipher
        .decrypt(&Nonce::default(), ciphertext.as_slice())
        .map_err(|_| anyhow!("decryption failed for {:?}", value))?;
    Ok(String::from_utf8(plaintext)?)
}

pub fn encrypt_path(value: &ArchivePath, cipher: &Aes256SivAead) -> Result<EncryptedArchivePath> {
    let parts = value
        .to_str_without_prefix()
        .split('/')
        .map(|part| {
            if part.is_empty() {
                Ok(String::new())
            } else {
                encrypt_str(part, cipher)
            }
        })
        .collect::<Result<Vec<String>>>()?;
    EncryptedArchivePath::from_encrypted_without_prefix(&parts.join("/"))
}

pub fn decrypt_path(value: &EncryptedArchivePath, cipher: &Aes256SivAead) -> Result<ArchivePath> {
    let parts = value
        .to_str_without_prefix()
        .split('/')
        .map(|part| {
            if part.is_empty() {
                Ok(String::new())
            } else {
                decrypt_str(part, cipher)
            }
        })
        .collect::<Result<Vec<String>>>()?;
    ArchivePath::from_str_without_prefix(&parts.join("/"))
}

pub fn encrypt_content_hash(
    value: &ContentHash,
    cipher: &Aes256SivAead,
) -> Result<EncryptedContentHash> {
    let ciphertext = cipher
        .encrypt(&Nonce::default(), value.as_slice())
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok(EncryptedContentHash::from_encrypted(ciphertext))
}

pub fn decrypt_content_hash(
    value: &EncryptedContentHash,
    cipher: &Aes256SivAead,
) -> Result<ContentHash> {
    cipher
        .decrypt(&Nonce::default(), value.as_slice())
        .map_err(|_| anyhow!("decryption failed for {:?}", value))?
        .try_into()
}

pub fn encrypt_size(value: u64, cipher: &Aes256SivAead) -> Result<EncryptedSize> {
    let ciphertext = cipher
        .encrypt(&Nonce::default(), &value.to_le_bytes()[..])
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok(EncryptedSize::from_encrypted(ciphertext))
}

pub fn decrypt_size(value: &EncryptedSize, cipher: &Aes256SivAead) -> Result<u64> {
    let plaintext = cipher
        .decrypt(&Nonce::default(), value.as_slice())
        .map_err(|_| anyhow!("decryption failed for {:?}", value))?;
    if plaintext.len() != 8 {
        bail!(
            "decrypt_size: invalid decrypted length: {}, expected 8",
            plaintext.len()
        );
    }
    Ok(u64::from_le_bytes(plaintext.try_into().unwrap()))
}

#[test]
pub fn str_roundtrip() {
    use aes_siv::KeyInit;

    let key = Aes256SivAead::generate_key(&mut OsRng);
    let cipher = Aes256SivAead::new(&key);
    let value = "abcd1";
    let encrypted = encrypt_str(value, &cipher).unwrap();
    assert_ne!(value, encrypted);
    let decrypted = decrypt_str(&encrypted, &cipher).unwrap();
    assert_eq!(value, decrypted);
}

#[test]
pub fn path_roundtrip() {
    use aes_siv::KeyInit;

    let key = Aes256SivAead::generate_key(&mut OsRng);
    let cipher = Aes256SivAead::new(&key);
    let value: ArchivePath = "ar:/ab/cd/ef".parse().unwrap();
    let encrypted = encrypt_path(&value, &cipher).unwrap();
    assert_ne!(
        value.to_str_without_prefix(),
        encrypted.to_str_without_prefix()
    );
    let decrypted = decrypt_path(&encrypted, &cipher).unwrap();
    assert_eq!(value, decrypted);
}

#[test]
pub fn file_roundtrip() {
    use aes_siv::KeyInit;
    use std::io::{Read, Seek, SeekFrom};
    use tempfile::NamedTempFile;

    let key = Aes256SivAead::generate_key(&mut OsRng);
    let cipher = Aes256SivAead::new(&key);

    let mut file = NamedTempFile::new().unwrap();
    for _ in 0..10 {
        let input: Vec<u8> = (0..3000).map(|_| rand::random::<u8>()).collect();
        file.write_all(&input).unwrap();
    }
    file.flush().unwrap();

    let mut encrypted_file = encrypt_file(file.path(), &cipher).unwrap().file;
    println!(
        "encrypted size {}",
        encrypted_file.seek(SeekFrom::End(0)).unwrap()
    );
    encrypted_file.rewind().unwrap();
    let mut decrypted_file = NamedTempFile::new().unwrap();
    let mut decryptor = Decryptor::new(&cipher, &mut decrypted_file);
    io::copy(&mut encrypted_file, &mut decryptor).unwrap();
    decryptor.finish().unwrap();
    decrypted_file.flush().unwrap();

    file.rewind().unwrap();
    decrypted_file.rewind().unwrap();
    let mut buf1 = vec![0u8; 1024];
    let mut buf2 = vec![0u8; 1024];
    loop {
        let len1 = file.read(&mut buf1).unwrap();
        let len2 = decrypted_file.read(&mut buf2).unwrap();
        assert_eq!(len1, len2);
        assert_eq!(buf1[..len1], buf2[..len2]);
        if len1 == 0 {
            break;
        }
    }
}
