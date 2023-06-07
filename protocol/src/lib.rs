#![allow(clippy::collapsible_else_if)]

pub mod endpoints;
mod path;
pub mod util;

pub use crate::path::{with_prefix as serde_path_with_prefix, ArchivePath, EncryptedArchivePath};
use anyhow::bail;
use anyhow::Result;
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use derive_more::{From, Into};
use endpoints::AddVersion;
use serde::{Deserialize, Serialize};
use std::fmt;

pub type DateTimeUtc = chrono::DateTime<Utc>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct SourceId(i32);

impl SourceId {
    pub fn to_db(self) -> i32 {
        self.0
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, From, Into,
)]
pub struct EntryUpdateNumber(i64);

impl EntryUpdateNumber {
    pub fn to_db(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct SnapshotId(i32);

impl SnapshotId {
    pub fn to_db(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct EntryId(i64);

impl EntryId {
    pub fn to_db(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Into)]
pub struct ContentHash(Vec<u8>);

impl ContentHash {
    pub fn new(hash: [u8; 32]) -> Self {
        Self(hash.into())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for ContentHash {
    type Error = anyhow::Error;

    fn try_from(value: Vec<u8>) -> Result<Self> {
        if value.len() != 32 {
            bail!("invalid hash length");
        }
        Ok(Self(value))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EncryptedContentHash(Vec<u8>);

impl EncryptedContentHash {
    pub fn from_encrypted(value: Vec<u8>) -> Self {
        Self(value)
    }

    pub fn to_url_safe(&self) -> String {
        BASE64_URL_SAFE_NO_PAD.encode(&self.0)
    }

    pub fn from_url_safe(s: &str) -> Result<Self> {
        let bytes = BASE64_URL_SAFE_NO_PAD.decode(s)?;
        Ok(Self(bytes))
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Into)]
pub struct EncryptedSize(Vec<u8>);

impl EncryptedSize {
    pub fn from_encrypted(value: Vec<u8>) -> Self {
        Self(value)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RecordTrigger {
    Sync,
    Upload,
    Reset,
    Move,
    Remove,
}

impl TryFrom<i32> for RecordTrigger {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Sync),
            1 => Ok(Self::Upload),
            2 => Ok(Self::Reset),
            3 => Ok(Self::Move),
            4 => Ok(Self::Remove),
            _ => bail!("invalid value for RecordTrigger: {}", value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    File = 1,
    Directory = 2,
}

impl EntryKind {
    pub const NOT_EXISTS: i32 = 0;
}

pub fn entry_kind_from_db(value: i32) -> Result<Option<EntryKind>> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(EntryKind::File)),
        2 => Ok(Some(EntryKind::Directory)),
        _ => bail!("invalid value for EntryKind: {}", value),
    }
}

pub fn entry_kind_to_db(value: Option<EntryKind>) -> i32 {
    match value {
        None => 0,
        Some(EntryKind::File) => 1,
        Some(EntryKind::Directory) => 2,
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EntryVersionData {
    pub path: EncryptedArchivePath,
    pub recorded_at: DateTimeUtc,
    pub source_id: SourceId,
    pub record_trigger: RecordTrigger,
    pub kind: Option<EntryKind>,
    pub content: Option<FileContent>,
}

impl EntryVersionData {
    pub fn is_same(&self, update: &AddVersion) -> bool {
        self.path == update.path && self.kind == update.kind && {
            match (&self.content, &update.content) {
                (Some(content), Some(update)) => {
                    content.hash == update.hash
                        && match (content.unix_mode, update.unix_mode) {
                            (None, None) => true,
                            (None, Some(_)) => false,
                            (Some(_), None) => true,
                            (Some(mode1), Some(mode2)) => mode1 == mode2,
                        }
                }
                (None, None) => true,
                _ => false,
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: EntryId,
    pub update_number: EntryUpdateNumber,
    pub parent_dir: Option<EntryId>,
    pub data: EntryVersionData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EntryVersion {
    pub entry_id: EntryId,
    pub snapshot_id: Option<SnapshotId>,
    pub data: EntryVersionData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub modified_at: DateTimeUtc,
    pub original_size: EncryptedSize,
    pub encrypted_size: u64,
    pub hash: EncryptedContentHash,
    pub unix_mode: Option<u32>,
}
