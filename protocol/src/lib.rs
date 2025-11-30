#![allow(clippy::collapsible_else_if)]

mod credentials;
pub mod encoding;
pub mod endpoints;
mod path;
pub mod util;

pub use crate::{
    credentials::{AccessToken, EncryptionKey},
    path::{with_prefix as serde_path_with_prefix, ArchivePath, EncryptedArchivePath},
};

use {
    anyhow::{bail, Result},
    base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine},
    chrono::Utc,
    derive_more::{From, Into},
    endpoints::AddVersion,
    serde::{Deserialize, Serialize},
    std::fmt,
};

pub type DateTimeUtc = chrono::DateTime<Utc>;

/// Identifier of a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct SourceId(i32);

impl SourceId {
    pub fn to_db(self) -> i32 {
        self.0
    }
}

/// Number of an entry update.
///
/// `EntryUpdateNumber` is based on a global counter that increments every time an entry is updated.
/// It's used to request new updates from the server based on the last `EntryUpdateNumber` seen by the client.
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

/// SHA-256 hash of unencrypted content of a file.
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

/// Encrypted value of `ContentHash`.
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

/// Encrypted value of file size.
///
/// File size in bytes is encoded as u64 LE before encrypting it.
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

/// Action that caused an entity update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RecordTrigger {
    Sync,
    Upload,
    Reset,
    Move,
    Remove,
}

impl RecordTrigger {
    pub fn to_db(self) -> i32 {
        match self {
            RecordTrigger::Sync => 0,
            RecordTrigger::Upload => 1,
            RecordTrigger::Reset => 2,
            RecordTrigger::Move => 3,
            RecordTrigger::Remove => 4,
        }
    }

    pub fn from_db(value: i32) -> anyhow::Result<Self> {
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
    pub fn to_db(self) -> i32 {
        match self {
            EntryKind::File => 1,
            EntryKind::Directory => 2,
        }
    }
}

impl EntryKind {
    /// Database value for a non-existing entry.
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
        Some(value) => value.to_db(),
    }
}

/// Data associated with an entry at a particular time.
#[derive(Debug, Serialize, Deserialize)]
pub struct EntryVersionData {
    /// Encrypted path of the entry (never changes).
    pub path: EncryptedArchivePath,
    /// Time of recording this version.
    pub recorded_at: DateTimeUtc,
    /// ID of the client that created this version.
    pub source_id: SourceId,
    /// Action that caused an entity update (as reported by the client).
    pub record_trigger: RecordTrigger,
    /// Kind of the entry (file or directory), or `None` if this version
    /// records a deletion of this entry.
    pub kind: Option<EntryKind>,
    /// File content (only allowed if `kind == Some(File)`).
    pub content: Option<FileContent>,
}

fn is_same_or_unknown<T: PartialEq>(old: Option<T>, new: Option<T>) -> bool {
    match (old, new) {
        // Unknown in old and new, no need to record it.
        (None, None) => true,
        // New known value, we need to record it.
        (None, Some(_)) => false,
        // No new known value, no need to record it.
        (Some(_), None) => true,
        // Old and new are known values, we need to record it if it's different.
        (Some(mode1), Some(mode2)) => mode1 == mode2,
    }
}

impl EntryVersionData {
    /// Checks if `AddVersion` is an update compared to `self`.
    ///
    /// This is just an equality check for the most part, but it includes
    /// special handling of `unix_mode`.
    pub fn is_same(&self, update: &AddVersion) -> bool {
        self.path == update.path && self.kind == update.kind && {
            match (&self.content, &update.content) {
                (Some(content), Some(update)) => {
                    content.hash == update.hash
                        && is_same_or_unknown(content.unix_mode, update.unix_mode)
                        && is_same_or_unknown(content.is_symlink, update.is_symlink)
                }
                (None, None) => true,
                _ => false,
            }
        }
    }
}

/// State of the archive at a particular encrypted archive path.
#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: EntryId,
    /// Update number corresponding to the last update of this entry.
    pub update_number: EntryUpdateNumber,
    /// ID of the parent entry. Is `None` only for the root path (`ar:/`).
    pub parent_dir: Option<EntryId>,
    /// Current data for the entry.
    pub data: EntryVersionData,
}

/// State of an entry at a particular point in time.
#[derive(Debug, Serialize, Deserialize)]
pub struct EntryVersion {
    pub entry_id: EntryId,
    /// Only present if this entry version belongs to a snapshot.
    pub snapshot_id: Option<SnapshotId>,
    /// Data of the entry at this version.
    pub data: EntryVersionData,
}

/// Encrypted record of a file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub modified_at: DateTimeUtc,
    /// Encrypted value of the size of the unencrypted file content in bytes.
    pub original_size: EncryptedSize,
    /// Size of the encrypted file content in bytes.
    pub encrypted_size: u64,
    /// Encrypted value of the SHA-256 hash of the unencrypted file content.
    pub hash: EncryptedContentHash,
    /// Unix mode of the file. Absent if unix mode is not available on the system
    /// that generated this `FileContent` value.
    pub unix_mode: Option<u32>,
    /// `Some(true)` if this file is a symlink. `Some(false)` if it's a regular file.
    /// `None` if symlinks are not supported on the system that generated this `FileContent` value.
    pub is_symlink: Option<bool>,
}
