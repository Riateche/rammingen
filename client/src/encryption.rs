use aes_siv::aead::Aead;
use aes_siv::AeadCore;
use aes_siv::{aead::OsRng, Aes256SivAead, Nonce};
use anyhow::{anyhow, Result};
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use byteorder::{ByteOrder, WriteBytesExt, LE};
use deflate::write::DeflateEncoder;
use deflate::CompressionOptions;
use fs_err::File;
use inflate::InflateWriter;
use rammingen_protocol::{ArchivePath, ContentHash};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::cmp::min;
use std::io::{self, Write};
use std::path::Path;
use tempfile::SpooledTempFile;
use typenum::ToInt;

const BLOCK_SIZE: usize = 1024 * 1024;
const MAX_IN_MEMORY: usize = 32 * 1024 * 1024;
const MAGIC_NUMBER: u32 = 3137690536;

fn nonce_size() -> usize {
    <Aes256SivAead as AeadCore>::NonceSize::to_int()
}

struct HashingWriter<W> {
    hasher: Sha256,
    size: u64,
    inner: W,
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

impl<W> HashingWriter<W> {
    pub fn new(inner: W, salt: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(salt);
        Self {
            hasher,
            inner,
            size: 0,
        }
    }
}

struct EncryptingWriter<'a, W> {
    buf: Vec<u8>,
    output: W,
    cipher: &'a Aes256SivAead,
}

impl<'a, W: Write> EncryptingWriter<'a, W> {
    fn finish(mut self) -> io::Result<W> {
        self.write_block()?;
        self.output.flush()?;
        Ok(self.output)
    }
}

impl<'a, W: Write> EncryptingWriter<'a, W> {
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

        self.buf.drain(..input_len);

        Ok(())
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
    pub size: u64,
}

pub fn encrypt_file(path: &Path, cipher: &Aes256SivAead, salt: &str) -> Result<EncryptedFileData> {
    let mut file = File::open(path)?;
    let mut output = SpooledTempFile::new(MAX_IN_MEMORY);
    output.write_u32::<LE>(MAGIC_NUMBER)?;
    let mut encryptor = EncryptingWriter {
        buf: Vec::new(),
        output: &mut output,
        cipher,
    };
    let mut encoder = DeflateEncoder::new(&mut encryptor, CompressionOptions::high());
    let mut hasher = HashingWriter::new(&mut encoder, salt);
    io::copy(&mut file, &mut hasher)?;
    let hash = ContentHash(hasher.hasher.finalize().to_vec());
    let size = hasher.size;
    encoder.finish()?;
    encryptor.finish()?;
    Ok(EncryptedFileData {
        file: output,
        hash,
        size,
    })
}

pub struct Decryptor<'a, W: Write> {
    got_header: bool,
    buf: Vec<u8>,
    cipher: &'a Aes256SivAead,
    output: InflateWriter<W>,
}

impl<'a, W: Write> Decryptor<'a, W> {
    pub fn new(cipher: &'a Aes256SivAead, output: W) -> Self {
        Self {
            got_header: false,
            buf: Vec::new(),
            cipher,
            output: InflateWriter::new(output),
        }
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.process_block()?;
        if !self.buf.is_empty() {
            return Err(io::Error::new(io::ErrorKind::Other, "trailing data found"));
        }
        self.output.finish()
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
        let len = usize::try_from(LE::read_u32(&self.buf))
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
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok(String::from_utf8(plaintext)?)
}

pub fn encrypt_path(value: &ArchivePath, cipher: &Aes256SivAead) -> Result<ArchivePath> {
    let parts = value
        .0
        .split('/')
        .map(|part| {
            if part.is_empty() {
                Ok(String::new())
            } else {
                encrypt_str(part, cipher)
            }
        })
        .collect::<Result<Vec<String>>>()?;
    ArchivePath::from_str_without_prefix(&parts.join("/"))
}

pub fn decrypt_path(value: &ArchivePath, cipher: &Aes256SivAead) -> Result<ArchivePath> {
    let parts = value
        .0
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
    assert_ne!(value, encrypted);
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

    let mut encrypted_file = encrypt_file(file.path(), &cipher, "salt").unwrap().file;
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