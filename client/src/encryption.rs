use crate::config::EncryptionKey;
use aes_siv::aead::Aead;
use aes_siv::AeadCore;
use aes_siv::{
    aead::{AeadInPlace, KeyInit, OsRng},
    Aes256SivAead,
    Nonce, // Or `Aes128SivAead`
};
use anyhow::{anyhow, bail, Result};
use byteorder::{ByteOrder, WriteBytesExt, LE};
use fs_err::File;
use rand::RngCore;
use std::io::{Seek, SeekFrom, Write};
use std::{io::Read, path::Path};
use tempfile::{NamedTempFile, SpooledTempFile};
use typenum::ToInt;

const BLOCK_SIZE: usize = 1024 * 1024;
const MAX_IN_MEMORY: usize = 32 * 1024 * 1024;

fn nonce_size() -> usize {
    <Aes256SivAead as AeadCore>::NonceSize::to_int()
}

pub fn encrypt_file(path: &Path, cipher: &Aes256SivAead) -> Result<SpooledTempFile> {
    let mut file = File::open(path)?;
    let mut buf = vec![0; BLOCK_SIZE];
    let mut output = SpooledTempFile::new(MAX_IN_MEMORY);
    let mut nonce = Nonce::default();
    loop {
        let input_len = file.read(&mut buf)?;
        if input_len == 0 {
            break;
        }
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(&nonce, &buf[..input_len])
            .map_err(|_| anyhow!("encryption failed"))?;
        let output_size = nonce.len() + ciphertext.len();

        output.write_u32::<LE>(output_size as u32)?;
        output.write_all(&nonce)?;
        output.write_all(&ciphertext)?;
    }
    Ok(output)
}

pub fn decrypt_file_chunk(
    chunk: &[u8],
    cipher: &Aes256SivAead,
    output: &mut impl Write,
) -> Result<()> {
    if chunk.len() < 4 {
        bail!("chunk is too short");
    }
    let len = usize::try_from(LE::read_u32(chunk))?;
    let chunk_data = &chunk[4..];
    if len != chunk_data.len() {
        bail!("chunk length mismatch");
    }
    let nonce_size = nonce_size();
    let nonce = chunk_data
        .get(..nonce_size)
        .ok_or_else(|| anyhow!("chunk data is too short"))?;
    let nonce = Nonce::from_slice(nonce);
    let plaintext = cipher
        .decrypt(nonce, &chunk_data[nonce_size..])
        .map_err(|_| anyhow!("decryption failed"))?;
    output.write_all(&plaintext)?;
    Ok(())
}

#[test]
pub fn file_roundtrip() {
    let key = Aes256SivAead::generate_key(&mut OsRng);
    let cipher = Aes256SivAead::new(&key);

    let mut file = NamedTempFile::new().unwrap();
    for _ in 0..10 {
        let input: Vec<u8> = (0..3000).map(|_| rand::random::<u8>()).collect();
        file.write_all(&input).unwrap();
    }
    file.flush().unwrap();

    let mut encrypted_file = encrypt_file(file.path(), &cipher).unwrap();
    encrypted_file.seek(SeekFrom::Start(0)).unwrap();
    let mut buf = Vec::new();
    let mut decrypted_file = NamedTempFile::new().unwrap();
    loop {
        buf.resize(4, 0);
        let len = encrypted_file.read(&mut buf).unwrap();
        if len == 0 {
            break;
        } else if len != 4 {
            panic!("unexpected eof");
        }
        let size = LE::read_u32(&buf) as usize;
        buf.resize(4 + size, 0);
        encrypted_file.read_exact(&mut buf[4..]).unwrap();
        decrypt_file_chunk(&buf, &cipher, &mut decrypted_file).unwrap();
    }
    decrypted_file.flush().unwrap();

    file.seek(SeekFrom::Start(0)).unwrap();
    decrypted_file.seek(SeekFrom::Start(0)).unwrap();
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
