use std::{fmt, str::FromStr};

use anyhow::bail;
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use derive_more::{From, Into};
use serde::{Deserialize, Serialize};

pub type DateTime = chrono::DateTime<Utc>;

pub const VERSION: u32 = 1;

pub trait RequestToResponse {
    type Response;
}
macro_rules! response_type {
    ($request:ty, $response:ty) => {
        impl RequestToResponse for $request {
            type Response = $response;
        }
    };
}

pub trait RequestToStreamingResponse {
    type ResponseItem;
}
macro_rules! streaming_response_type {
    ($request:ty, $response:ty) => {
        impl RequestToStreamingResponse for $request {
            type ResponseItem = $response;
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
    GetContentHead(GetContentHead),
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
        if bytes.len() != 64 {
            bail!("invalid hash length");
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
    pub path: String,
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
    pub path: String,
}
streaming_response_type!(GetVersions, Vec<EntryVersion>);

// Returns all versions
#[derive(Debug, Serialize, Deserialize)]
pub struct GetAllVersions {
    // if it's a dir, return all versions for each nested path
    pub path: String,
}
streaming_response_type!(GetAllVersions, Vec<EntryVersion>);

#[derive(Debug, Serialize, Deserialize)]
pub struct AddVersion {
    pub path: String,
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
    pub path: String,
    pub recorded_at: Option<DateTime>,
}
response_type!(ResetVersion, BulkActionStats);

#[derive(Debug, Serialize, Deserialize)]
pub struct MovePath {
    pub old_path: String,
    pub new_path: String,
}
response_type!(MovePath, BulkActionStats);

#[derive(Debug, Serialize, Deserialize)]
pub struct RemovePath {
    pub path: String,
}
response_type!(RemovePath, BulkActionStats);

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveVersion {
    // if dir, remove this version for all nested paths (where it's present)
    pub path: String,
    pub recorded_at: Option<DateTime>,
}
response_type!(RemoveVersion, BulkActionStats);

#[derive(Debug, Serialize, Deserialize)]
pub struct GetContentHead {
    pub content_hash: ContentHash,
}
response_type!(GetContentHead, Option<ContentHead>);

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
