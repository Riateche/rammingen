use {
    crate::crypto::Cipher,
    anyhow::{Context as _, Result},
    rammingen_protocol::{
        ArchivePath, ContentHash, DateTimeUtc, EntryKind, EntryState, EntryVersionData,
        RecordTrigger, SourceId,
    },
    serde::{Deserialize, Serialize},
    std::path::Path,
    tempfile::SpooledTempFile,
};

/// Metadata of a local file content or decoded metadata of a file content fetched from server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalFileEntry {
    pub modified_at: DateTimeUtc,
    pub original_size: u64,
    pub encrypted_size: u64,
    pub hash: ContentHash,
    /// Only present if supported on current OS.
    pub unix_mode: Option<u32>,
    /// Only present if supported on current OS.
    pub is_symlink: Option<bool>,
}

/// Metadata of a local file or directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalEntry {
    pub kind: EntryKind,
    /// Only allowed if `kind == EntryKind::File`.
    pub file_data: Option<LocalFileEntry>,
}

impl LocalEntry {
    /// Returns whether local `path` has the same entry kind and modified time (for files only)
    /// as `self`. Doesn't compare the other file content data (content hash, size, unix mode, etc.).
    #[inline]
    pub fn matches_real(&self, path: impl AsRef<Path>) -> Result<bool> {
        let metadata = fs_err::symlink_metadata(path)?;
        if metadata.is_dir() != (self.kind == EntryKind::Directory) {
            return Ok(false);
        }
        if self.kind == EntryKind::File {
            let content = self
                .file_data
                .as_ref()
                .context("missing content for file")?;
            if DateTimeUtc::from(metadata.modified()?) != content.modified_at {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Decrypted data of an entry version received from the server.
///
/// It represents the state of an archive path at some point in time.
#[derive(Debug, Serialize, Deserialize)]
pub struct LocalArchiveEntry {
    pub path: ArchivePath,
    /// Server-side update timestamp.
    pub recorded_at: DateTimeUtc,
    /// ID of the client that created this entry version.
    pub source_id: SourceId,
    /// Type of operation that created this entry version.
    pub record_trigger: RecordTrigger,
    /// State of the file node.
    pub state: EntryState,
    /// File content properties (only if `state == EntryState::Exists(EntryKind::File)`).
    pub file_data: Option<LocalFileEntry>,
}

impl LocalArchiveEntry {
    /// Convert `EntryVersionData` to `LocalArchiveEntry`.
    #[inline]
    pub fn decrypt(data: EntryVersionData, cipher: &Cipher) -> Result<Self> {
        Ok(Self {
            path: cipher.decrypt_path(&data.path)?,
            recorded_at: data.recorded_at,
            source_id: data.source_id,
            record_trigger: data.record_trigger,
            state: data.state,
            file_data: if let Some(content) = data.content {
                Some(LocalFileEntry {
                    modified_at: content.modified_at,
                    original_size: cipher.decrypt_size(&content.original_size)?,
                    encrypted_size: content.encrypted_size,
                    hash: cipher.decrypt_content_hash(&content.hash)?,
                    unix_mode: content.unix_mode,
                    is_symlink: content.is_symlink,
                })
            } else {
                None
            },
        })
    }
}

/// Encrypted file content and metadata.
pub struct TemporaryEncryptedFile {
    /// Temporary file that contains the encrypted content.
    pub file: SpooledTempFile,
    /// Unencrypted content hash.
    pub hash: ContentHash,
    /// Size of the unencrypted content in bytes.
    pub original_size: u64,
    /// Size of the encrypted content in bytes.
    pub encrypted_size: u64,
}
