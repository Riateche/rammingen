#![allow(clippy::collapsible_else_if)]

use std::{fmt, str::FromStr};

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use derive_more::{From, Into};
use serde::{de::Error, Deserialize, Serialize};
use util::check_path;

pub mod util;

pub type DateTime = chrono::DateTime<Utc>;

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ArchivePath(pub String);

impl ArchivePath {
    pub fn from_str_without_prefix(path: &str) -> Result<Self> {
        check_path(path)?;
        Ok(Self(path.into()))
    }

    pub fn join(&self, file_name: &str) -> Result<ArchivePath> {
        if file_name.is_empty() {
            bail!("file name cannot be empty");
        }
        if file_name.contains('/') {
            bail!("file name cannot contain '/'");
        }
        let s = format!("{}/{}", self.0, file_name);
        check_path(&s)?;
        Ok(Self(s))
    }

    pub fn parent(&self) -> Option<ArchivePath> {
        if self.0 == "/" {
            None
        } else {
            let pos = self.0.rfind('/').expect("any path must contain '/'");
            let parent = if pos == 0 { "/" } else { &self.0[..pos] };
            check_path(parent).expect("parent should always be valid");
            Some(Self(parent.into()))
        }
    }
}

#[test]
fn parent_path() {
    assert_eq!(ArchivePath::from_str("ar:/").unwrap().parent(), None);
    assert_eq!(
        ArchivePath::from_str("ar:/ab").unwrap().parent(),
        Some(ArchivePath::from_str("ar:/").unwrap())
    );
    assert_eq!(
        ArchivePath::from_str("ar:/ab/cd").unwrap().parent(),
        Some(ArchivePath::from_str("ar:/ab").unwrap())
    );
}

impl<'de> Deserialize<'de> for ArchivePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        check_path(&s).map_err(D::Error::custom)?;
        Ok(Self(s))
    }
}

impl FromStr for ArchivePath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let path = s
            .strip_prefix("ar:")
            .ok_or_else(|| anyhow!("archive path must start with 'ar:'"))?;
        check_path(path)?;
        Ok(Self(path.into()))
    }
}

impl fmt::Display for ArchivePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ar:{}", self.0)
    }
}

pub trait RequestToResponse {
    type Response;
    const NAME: &'static str;
}
macro_rules! response_type {
    ($request:ty, $response:ty) => {
        impl RequestToResponse for $request {
            type Response = $response;
            const NAME: &'static str = stringify!($request);
        }
    };
}

pub trait RequestToStreamingResponse {
    type ResponseItem;
    const NAME: &'static str;
}
macro_rules! streaming_response_type {
    ($request:ty, $response:ty) => {
        impl RequestToStreamingResponse for $request {
            type ResponseItem = $response;
            const NAME: &'static str = stringify!($request);
        }
    };
}

pub type Response<Request> = <Request as RequestToResponse>::Response;
pub type StreamingResponseItem<Request> = <Request as RequestToStreamingResponse>::ResponseItem;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    GetEntries(GetEntries),
    GetVersions(GetVersions),
    GetAllVersions(GetAllVersions),
    AddVersion(AddVersion),
    ResetVersion(ResetVersion),
    MovePath(MovePath),
    RemovePath(RemovePath),
    RemoveVersion(RemoveVersion),
    GetContentHead(ContentHashExists),
    GetContent(GetContent),
    StartContentUpload(StartContentUpload),
    UploadContentChunk(UploadContentChunk),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct SourceId(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct EntryUpdateNumber(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct SnapshotId(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct EntryId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into)]
pub struct VersionId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, From, Into)]
pub struct ContentHash(pub Vec<u8>);

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", BASE64_URL_SAFE_NO_PAD.encode(&self.0))
    }
}

impl FromStr for ContentHash {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = BASE64_URL_SAFE_NO_PAD.decode(s)?;
        if bytes.len() != 32 {
            bail!("invalid hash length: expected 32, got {}", bytes.len());
        }
        Ok(Self(bytes))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Login {
    pub version: u32,
    pub source_id: SourceId,
    pub secret: String,
}
response_type!(Login, ());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RecordTrigger {
    Sync,
    Upload,
    Reset,
}

impl TryFrom<i32> for RecordTrigger {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Sync),
            1 => Ok(Self::Upload),
            2 => Ok(Self::Reset),
            _ => bail!("invalid value for RecordTrigger: {}", value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Directory,
}

impl TryFrom<i32> for EntryKind {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::File),
            1 => Ok(Self::Directory),
            _ => bail!("invalid value for EntryKind: {}", value),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EntryVersionData {
    pub path: ArchivePath,
    pub recorded_at: DateTime,
    pub source_id: SourceId,
    pub record_trigger: RecordTrigger,
    pub kind: EntryKind,
    pub exists: bool,
    pub content: Option<FileContent>,
}

impl EntryVersionData {
    pub fn is_same(&self, update: &AddVersion) -> bool {
        self.path == update.path && self.kind == update.kind && self.exists == update.exists && {
            match (&self.content, &update.content) {
                (Some(content), Some(update)) => {
                    content.size == update.size
                        && content.content_hash == update.content_hash
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
    pub id: VersionId,
    pub entry_id: EntryId,
    pub snapshot_id: Option<SnapshotId>,
    pub data: EntryVersionData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileContent {
    pub modified_at: DateTime,
    pub size: u64,
    pub content_hash: ContentHash,
    pub unix_mode: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetEntries {
    // for incremental updates
    pub last_update_number: Option<EntryUpdateNumber>,
}
streaming_response_type!(GetEntries, Vec<Entry>);

// Returns the closest version to the specified date
#[derive(Debug, Serialize, Deserialize)]
pub struct GetVersions {
    pub recorded_at: DateTime,
    // if it's a dir, return a version for each nested path
    pub path: ArchivePath,
}
streaming_response_type!(GetVersions, Vec<EntryVersion>);

// Returns all versions
#[derive(Debug, Serialize, Deserialize)]
pub struct GetAllVersions {
    // if it's a dir, return all versions for each nested path
    pub path: ArchivePath,
}
streaming_response_type!(GetAllVersions, Vec<EntryVersion>);

#[derive(Debug, Serialize, Deserialize)]
pub struct AddVersion {
    pub path: ArchivePath,
    pub record_trigger: RecordTrigger,
    pub kind: EntryKind,
    pub exists: bool,
    pub content: Option<FileContent>,
}
response_type!(AddVersion, Option<VersionId>);

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkActionStats {
    pub affected_paths: u64,
}

/// Set the specified version as the latest one.
/// If a directory, resets all nested paths.
#[derive(Debug, Serialize, Deserialize)]
pub struct ResetVersion {
    pub path: ArchivePath,
    pub recorded_at: Option<DateTime>,
}
response_type!(ResetVersion, BulkActionStats);

#[derive(Debug, Serialize, Deserialize)]
pub struct MovePath {
    pub old_path: ArchivePath,
    pub new_path: ArchivePath,
}
response_type!(MovePath, BulkActionStats);

#[derive(Debug, Serialize, Deserialize)]
pub struct RemovePath {
    pub path: ArchivePath,
}
response_type!(RemovePath, BulkActionStats);

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveVersion {
    // if dir, remove this version for all nested paths (where it's present)
    pub path: ArchivePath,
    pub recorded_at: Option<DateTime>,
}
response_type!(RemoveVersion, BulkActionStats);

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentHashExists(pub ContentHash);
response_type!(ContentHashExists, bool);

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentHead {
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetContent {
    pub content_hash: ContentHash,
}
response_type!(GetContent, Option<Vec<u8>>); // TODO: streaming

#[derive(Debug, Serialize, Deserialize)]
pub struct StartContentUpload {
    pub content_hash: ContentHash,
    pub size: u64,
}
response_type!(StartContentUpload, ());

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadContentChunk(Option<Vec<u8>>);
response_type!(UploadContentChunk, ());
