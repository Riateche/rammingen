use aes_siv::aead::Aead;
use aes_siv::AeadCore;
use aes_siv::{aead::OsRng, Aes256SivAead, Nonce};
use anyhow::Result;
use byteorder::{ByteOrder, WriteBytesExt, LE};
use deflate::write::DeflateEncoder;
use deflate::CompressionOptions;
use fs_err::File;
use inflate::InflateWriter;
use rand::RngCore;
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

pub fn encrypt_file(path: &Path, cipher: &Aes256SivAead) -> Result<SpooledTempFile> {
    let mut file = File::open(path)?;
    let mut output = SpooledTempFile::new(MAX_IN_MEMORY);
    output.write_u32::<LE>(MAGIC_NUMBER)?;
    let writer = EncryptingWriter {
        buf: Vec::new(),
        output,
        cipher,
    };
    let mut encoder = DeflateEncoder::new(writer, CompressionOptions::high());
    io::copy(&mut file, &mut encoder)?;
    let writer = encoder.finish()?;
    Ok(writer.finish()?)
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

    let mut encrypted_file = encrypt_file(file.path(), &cipher).unwrap();
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
