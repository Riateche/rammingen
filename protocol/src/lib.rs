mod credentials;
pub mod encoding;
pub mod endpoints;
mod path;
pub mod util;

pub use crate::{
    credentials::{AccessToken, EncryptionKey},
    path::{ArchivePath, EncryptedArchivePath, with_prefix as serde_path_with_prefix},
};

use {
    anyhow::{Result, bail},
    base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD},
    chrono::Utc,
    derive_more::{From, Into},
    serde::{Deserialize, Serialize},
    std::fmt,
};

pub type DateTimeUtc = chrono::DateTime<Utc>;

/// Identifier of a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct SourceId(i32);

impl SourceId {
    /// Returns database representation.
    #[must_use]
    #[inline]
    pub fn to_db(self) -> i32 {
        self.0
    }
}

/// Number of an entry update.
///
/// `EntryUpdateNumber` is based on a global counter that increments every time any entry is updated.
/// It's used to request new updates from the server based on the last `EntryUpdateNumber` seen by the client.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, From, Into,
)]
pub struct EntryUpdateNumber(i64);

impl EntryUpdateNumber {
    /// Returns database representation.
    #[must_use]
    #[inline]
    pub fn to_db(self) -> i64 {
        self.0
    }
}

/// ID of a snapshot.
///
/// The server creates snapshots on a configured time interval.
/// This ID is used to identify entries that belong to the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct SnapshotId(i32);

impl SnapshotId {
    /// Returns database representation.
    #[must_use]
    #[inline]
    pub fn to_db(self) -> i32 {
        self.0
    }
}

/// ID of an entry.
///
/// An entry is created for each distinct encrypted archive path.
/// Although `EncryptedArchivePath` would be a unique identifier of an entry,
/// `EntryId` is used instead for better performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct EntryId(i64);

impl EntryId {
    /// Returns database representation.
    #[must_use]
    #[inline]
    pub fn to_db(self) -> i64 {
        self.0
    }
}

/// SHA-256 hash of unencrypted content of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Into)]
pub struct ContentHash(Vec<u8>);

impl ContentHash {
    #[must_use]
    #[inline]
    pub fn new(hash: [u8; 32]) -> Self {
        Self(hash.into())
    }

    #[must_use]
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for ContentHash {
    type Error = anyhow::Error;

    #[inline]
    fn try_from(value: Vec<u8>) -> Result<Self> {
        if value.len() != 32 {
            bail!("invalid hash length");
        }
        Ok(Self(value))
    }
}

impl fmt::Display for ContentHash {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.0))
    }
}

/// Encrypted value of `ContentHash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EncryptedContentHash(Vec<u8>);

impl EncryptedContentHash {
    #[must_use]
    #[inline]
    pub fn from_encrypted(value: Vec<u8>) -> Self {
        Self(value)
    }

    #[must_use]
    #[inline]
    pub fn to_url_safe(&self) -> String {
        BASE64_URL_SAFE_NO_PAD.encode(&self.0)
    }

    #[inline]
    pub fn from_url_safe(s: &str) -> Result<Self> {
        let bytes = BASE64_URL_SAFE_NO_PAD.decode(s)?;
        Ok(Self(bytes))
    }

    #[must_use]
    #[inline]
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
    #[must_use]
    #[inline]
    pub fn from_encrypted(value: Vec<u8>) -> Self {
        Self(value)
    }

    #[must_use]
    #[inline]
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
    /// Returns database representation.
    #[must_use]
    #[inline]
    pub fn to_db(self) -> i32 {
        match self {
            RecordTrigger::Sync => 0,
            RecordTrigger::Upload => 1,
            RecordTrigger::Reset => 2,
            RecordTrigger::Move => 3,
            RecordTrigger::Remove => 4,
        }
    }

    /// Convert from database representation.
    #[inline]
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

/// Type of the existing file node (file or directory).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    /// A regular file or a symlink.
    File = 1,
    /// A directory.
    Directory = 2,
}

impl EntryKind {
    /// Returns database representation.
    #[must_use]
    #[inline]
    pub fn to_db(self) -> i32 {
        match self {
            EntryKind::File => 1,
            EntryKind::Directory => 2,
        }
    }
}

/// State of the file node corresponding to an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryState {
    /// File or directory no longer exists.
    NotExists,
    /// File or directory exists.
    Exists(EntryKind),
}

// for compatibility with old encoding
impl<'de> Deserialize<'de> for EntryState {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let option = Option::<EntryKind>::deserialize(deserializer)?;
        match option {
            Some(kind) => Ok(Self::Exists(kind)),
            None => Ok(Self::NotExists),
        }
    }
}

// for compatibility with old encoding
impl Serialize for EntryState {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let option = match self {
            EntryState::NotExists => None,
            EntryState::Exists(kind) => Some(kind),
        };
        option.serialize(serializer)
    }
}

impl EntryState {
    /// Convert from database representation.
    #[inline]
    pub fn from_db(value: i32) -> Result<Self> {
        match value {
            0 => Ok(EntryState::NotExists),
            1 => Ok(EntryState::Exists(EntryKind::File)),
            2 => Ok(EntryState::Exists(EntryKind::Directory)),
            _ => bail!("invalid value for EntryKind: {}", value),
        }
    }

    /// Returns database representation.
    #[must_use]
    #[inline]
    pub fn to_db(&self) -> i32 {
        match self {
            EntryState::NotExists => 0,
            EntryState::Exists(value) => value.to_db(),
        }
    }

    /// Returns whether file or directory corresponding to this entry exists.
    #[must_use]
    #[inline]
    pub fn exists(&self) -> bool {
        matches!(self, Self::Exists(_))
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
    /// State of the file node.
    pub state: EntryState,
    /// File or symlink content (only allowed if `state == EntryState::Exists(EntryKind::File)`).
    pub content: Option<FileContent>,
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

/// Encrypted record of a file or symlink content.
///
/// For symlinks, the "content" is the target path of the symlink encoded in UTF-8.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    /// Filesystem last modification timestamp.
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
