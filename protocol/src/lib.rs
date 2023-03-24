use chrono::Utc;
use serde::{Deserialize, Serialize};

pub type DateTime = chrono::DateTime<Utc>;

pub trait RequestVariant {
    type Response;
}
macro_rules! response_type {
    ($request:ty, $response:ty) => {
        impl RequestVariant for $request {
            type Response = $response;
        }
    };
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Login(Login),
    GetVersions(GetVersions),
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceId(pub i32);

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotId(pub i32);

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionId(pub i64);

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentHash(pub Vec<u8>);

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentUploadId(pub i32);

#[derive(Debug, Serialize, Deserialize)]
pub struct Login {
    pub source_id: SourceId,
    pub secret: String,
}
response_type!(Login, ());

#[derive(Debug, Serialize, Deserialize)]
pub enum FileRecordTrigger {
    Sync,
    Upload,
    Reset,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FileKind {
    File,
    Directory,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileVersion {
    pub version_id: VersionId,
    pub path: String,
    pub recorded_at: DateTime,
    pub source_id: SourceId,
    pub record_trigger: FileRecordTrigger,
    pub parent_dir: Option<VersionId>,
    pub snapshot_id: SnapshotId,
    pub kind: FileKind,
    pub content: Option<FileContent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileContent {
    pub modified_time: DateTime,
    pub size: u64,
    pub content_hash: ContentHash,
    pub unix_mode: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetVersions {
    // for incremental updates
    pub last_version_id: Option<VersionId>,
    // send None to get latest versions
    pub recorded_at: Option<DateTime>,
    // if supplied, only get this path and nested paths
    pub path: Option<String>,
}
response_type!(GetVersions, Option<Vec<FileVersion>>); // TODO: streaming

#[derive(Debug, Serialize, Deserialize)]
pub struct AddVersion {
    pub path: String,
    pub record_trigger: FileRecordTrigger,
    pub kind: FileKind,
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
