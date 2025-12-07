use {
    crate::{content::EncryptedFileHead, crypto::io::encrypt_file_content},
    aes_siv::{Aes256SivAead, KeyInit, Nonce, aead::Aead},
    anyhow::{Context, Result},
    base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD},
    cadd::prelude::IntoType,
    rammingen_protocol::{
        ArchivePath, ContentHash, EncryptedArchivePath, EncryptedContentHash, EncryptedSize,
        EncryptionKey,
    },
    std::{io::Read, mem::size_of},
};

pub struct Cipher {
    inner: Aes256SivAead,
}

impl Cipher {
    #[must_use]
    #[inline]
    pub fn new(key: &EncryptionKey) -> Self {
        Self {
            inner: Aes256SivAead::new(key.get()),
        }
    }

    #[inline]
    pub fn encrypt_bytes(&self, nonce: &Nonce, plaintext: &[u8]) -> Result<Vec<u8>> {
        self.inner
            .encrypt(nonce, plaintext)
            .context("encryption failed for bytes")
    }

    #[inline]
    pub fn decrypt_bytes(&self, nonce: &Nonce, ciphertext: &[u8]) -> Result<Vec<u8>> {
        self.inner
            .decrypt(nonce, ciphertext)
            .context("decryption failed for bytes")
    }

    #[inline]
    pub fn encrypt_str(&self, value: &str) -> Result<String> {
        let ciphertext = self
            .inner
            .encrypt(&Nonce::default(), value.as_bytes())
            .context("encryption failed")?;
        Ok(BASE64_URL_SAFE_NO_PAD.encode(ciphertext))
    }

    #[inline]
    pub fn decrypt_str(&self, value: &str) -> Result<String> {
        let ciphertext = BASE64_URL_SAFE_NO_PAD.decode(value)?;
        let plaintext = self
            .inner
            .decrypt(&Nonce::default(), ciphertext.as_slice())
            .with_context(|| format!("decryption failed for `{value}`"))?;
        Ok(String::from_utf8(plaintext)?)
    }

    #[inline]
    pub fn encrypt_path(&self, value: &ArchivePath) -> Result<EncryptedArchivePath> {
        let parts = value
            .to_str_without_prefix()
            .split('/')
            .map(|part| {
                if part.is_empty() {
                    Ok(String::new())
                } else {
                    self.encrypt_str(part)
                }
            })
            .collect::<Result<Vec<String>>>()?;
        EncryptedArchivePath::from_encrypted_without_prefix(&parts.join("/"))
    }

    #[inline]
    pub fn decrypt_path(&self, value: &EncryptedArchivePath) -> Result<ArchivePath> {
        let parts = value
            .to_str_without_prefix()
            .split('/')
            .map(|part| {
                if part.is_empty() {
                    Ok(String::new())
                } else {
                    self.decrypt_str(part)
                }
            })
            .collect::<Result<Vec<String>>>()?;
        ArchivePath::from_str_without_prefix(&parts.join("/"))
    }

    #[inline]
    pub fn encrypt_content_hash(&self, value: &ContentHash) -> Result<EncryptedContentHash> {
        let ciphertext = self
            .inner
            .encrypt(&Nonce::default(), value.as_slice())
            .context("encryption failed")?;
        Ok(EncryptedContentHash::from_encrypted(ciphertext))
    }

    #[inline]
    pub fn decrypt_content_hash(&self, value: &EncryptedContentHash) -> Result<ContentHash> {
        self.inner
            .decrypt(&Nonce::default(), value.as_slice())
            .with_context(|| format!("decryption failed for {value:?}"))?
            .try_into()
    }

    #[inline]
    pub fn encrypt_size(&self, value: u64) -> Result<EncryptedSize> {
        let ciphertext = self
            .inner
            .encrypt(&Nonce::default(), &value.to_le_bytes()[..])
            .context("encryption failed")?;
        Ok(EncryptedSize::from_encrypted(ciphertext))
    }

    #[inline]
    pub fn decrypt_size(&self, value: &EncryptedSize) -> Result<u64> {
        const SIZE_LENGTH: usize = size_of::<u64>();
        let plaintext = self
            .inner
            .decrypt(&Nonce::default(), value.as_slice())
            .with_context(|| format!("decryption failed for {value:?}"))?;

        Ok(u64::from_le_bytes(
            plaintext
                .try_into_type::<[u8; SIZE_LENGTH]>()
                .map_err(|vec| {
                    anyhow::format_err!(
                        "invalid decrypted length: {}, expected {}",
                        vec.len(),
                        SIZE_LENGTH
                    )
                })?,
        ))
    }

    #[inline]
    pub fn encrypt_file_content(&self, file_content: impl Read) -> Result<EncryptedFileHead> {
        encrypt_file_content(self, file_content)
    }
}

#[cfg(test)]
#[expect(
    clippy::default_numeric_fallback,
    clippy::indexing_slicing,
    reason = "test"
)]
mod tests {
    use {
        super::*,
        crate::crypto::DecryptingWriter,
        fs_err::File,
        std::io::{self, Read, Seek, SeekFrom, Write},
        tempfile::NamedTempFile,
    };

    #[test]
    pub fn str_roundtrip() {
        let key = EncryptionKey::generate().unwrap();
        let cipher = Cipher::new(&key);
        let value = "abcd1";
        let encrypted = cipher.encrypt_str(value).unwrap();
        assert_ne!(value, encrypted);
        let decrypted = cipher.decrypt_str(&encrypted).unwrap();
        assert_eq!(value, decrypted);
    }

    #[test]
    pub fn path_roundtrip() {
        let key = EncryptionKey::generate().unwrap();
        let cipher = Cipher::new(&key);
        let value: ArchivePath = "ar:/ab/cd/ef".parse().unwrap();
        let encrypted = cipher.encrypt_path(&value).unwrap();
        assert_ne!(
            value.to_str_without_prefix(),
            encrypted.to_str_without_prefix()
        );
        let decrypted = cipher.decrypt_path(&encrypted).unwrap();
        assert_eq!(value, decrypted);
    }

    #[test]
    pub fn file_roundtrip() {
        let key = EncryptionKey::generate().unwrap();
        let cipher = Cipher::new(&key);

        let mut file = NamedTempFile::new().unwrap();
        for _ in 0..20_000 {
            let input: Vec<u8> = (0..1000).map(|_| rand::random::<u8>()).collect();
            file.write_all(&input).unwrap();
        }
        file.flush().unwrap();

        let mut encrypted_file = cipher
            .encrypt_file_content(File::open(file.path()).unwrap())
            .unwrap();
        assert_eq!(encrypted_file.original_size, 20_000_000);
        println!(
            "encrypted size {}",
            encrypted_file.file.seek(SeekFrom::End(0)).unwrap(),
        );
        encrypted_file.file.rewind().unwrap();
        let mut decrypted_file = NamedTempFile::new().unwrap();
        let mut decryptor = DecryptingWriter::new(&cipher, &mut decrypted_file);
        io::copy(&mut encrypted_file.file, &mut decryptor).unwrap();
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
}
