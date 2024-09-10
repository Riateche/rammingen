use std::path::Path;

use anyhow::{anyhow, Result};
use rammingen_protocol::{
    ArchivePath, ContentHash, DateTimeUtc, EntryKind, EntryVersionData, RecordTrigger, SourceId,
};
use serde::{Deserialize, Serialize};
use tempfile::SpooledTempFile;

use crate::crypto::Cipher;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptedContentHead {
    pub modified_at: DateTimeUtc,
    pub original_size: u64,
    pub encrypted_size: u64,
    pub hash: ContentHash,
    pub unix_mode: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LocalEntry {
    pub kind: EntryKind,
    pub content: Option<DecryptedContentHead>,
}

impl LocalEntry {
    pub fn is_same_as_entry(&self, other: &DecryptedEntryVersion) -> bool {
        if Some(self.kind) != other.kind {
            return false;
        }
        match self.kind {
            EntryKind::File => match (&self.content, &other.content) {
                (Some(content), Some(other)) => {
                    if content.hash != other.hash {
                        return false;
                    }
                    match (content.unix_mode, other.unix_mode) {
                        (None, _) => true,
                        (Some(_), None) => true,
                        (Some(unix_mode), Some(other)) => unix_mode == other,
                    }
                }
                _ => false,
            },
            EntryKind::Directory => true,
        }
    }

    pub fn matches_real(&self, path: impl AsRef<Path>) -> Result<bool> {
        let metadata = fs_err::metadata(path)?;
        if metadata.is_symlink() {
            return Ok(false);
        }
        if metadata.is_dir() != (self.kind == EntryKind::Directory) {
            return Ok(false);
        }
        if self.kind == EntryKind::File {
            let content = self
                .content
                .as_ref()
                .ok_or_else(|| anyhow!("missing content for file"))?;
            if DateTimeUtc::from(metadata.modified()?) != content.modified_at {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DecryptedEntryVersion {
    pub path: ArchivePath,
    pub recorded_at: DateTimeUtc,
    pub source_id: SourceId,
    pub record_trigger: RecordTrigger,
    pub kind: Option<EntryKind>,
    pub content: Option<DecryptedContentHead>,
}

impl DecryptedEntryVersion {
    pub fn new(data: EntryVersionData, cipher: &Cipher) -> Result<Self> {
        Ok(Self {
            path: cipher.decrypt_path(&data.path)?,
            recorded_at: data.recorded_at,
            source_id: data.source_id,
            record_trigger: data.record_trigger,
            kind: data.kind,
            content: if let Some(content) = data.content {
                Some(DecryptedContentHead {
                    modified_at: content.modified_at,
                    original_size: cipher.decrypt_size(&content.original_size)?,
                    encrypted_size: content.encrypted_size,
                    hash: cipher.decrypt_content_hash(&content.hash)?,
                    unix_mode: content.unix_mode,
                })
            } else {
                None
            },
        })
    }
}

pub struct EncryptedFileHead {
    pub file: SpooledTempFile,
    pub hash: ContentHash,
    pub original_size: u64,
    pub encrypted_size: u64,
}
