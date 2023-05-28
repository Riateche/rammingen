use serde::{Deserialize, Serialize};

use crate::{
    DateTimeUtc, EncryptedArchivePath, EncryptedContentHash, Entry, EntryKind, EntryUpdateNumber,
    EntryVersion, FileContent, RecordTrigger, SourceId,
};

pub trait RequestToResponse {
    type Response;
    const PATH: &'static str;
}
macro_rules! response_type {
    ($request:ty, $response:ty) => {
        impl RequestToResponse for $request {
            type Response = $response;
            const PATH: &'static str = concat!("/api/v1/", stringify!($request));
        }
    };
}

pub trait RequestToStreamingResponse {
    type ResponseItem;
    const PATH: &'static str;
}
macro_rules! streaming_response_type {
    ($request:ty, $response:ty) => {
        impl RequestToStreamingResponse for $request {
            type ResponseItem = $response;
            const PATH: &'static str = concat!("/api/v1/", stringify!($request));
        }
    };
}

pub type Response<Request> = <Request as RequestToResponse>::Response;
pub type StreamingResponseItem<Request> = <Request as RequestToStreamingResponse>::ResponseItem;

/// Returns all entries added or updated since the specified update number.
/// Results are ordered by update number.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetNewEntries {
    // for incremental updates
    pub last_update_number: EntryUpdateNumber,
}
streaming_response_type!(GetNewEntries, Entry);

/// Returns all entries that are direct children of the specified path.
/// Results are ordered by path.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetDirectChildEntries(pub EncryptedArchivePath);
streaming_response_type!(GetDirectChildEntries, Entry);

/// Returns the version of the path corresponding to the specified time.
/// If it's a directory, also returns the version of each child path
/// at this time. Results are ordered by path.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetEntryVersionsAtTime {
    pub recorded_at: DateTimeUtc,
    pub path: EncryptedArchivePath,
}
streaming_response_type!(GetEntryVersionsAtTime, EntryVersion);

/// Returns all versions of the specified path.
/// If `recursive` is true, also returns all versions of all
/// nested paths. Results are ordered by `recorded_at`.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetAllEntryVersions {
    pub path: EncryptedArchivePath,
    pub recursive: bool,
}
streaming_response_type!(GetAllEntryVersions, EntryVersion);

/// Adds a new version of the specified path.
/// If `kind` is `None`, records deletion of the path.
/// `content` must be specified only if the entry is an existing file.
/// If `unix_mode` is not specified in `content`, the previous `unix_mode`
/// is preserved (if any).
/// Does nothing if the specified version is considered the same
/// as the last version of this path (`record_trigger` and `modified_at`
/// do not count as meaningful changes).
#[derive(Debug, Serialize, Deserialize)]
pub struct AddVersion {
    pub path: EncryptedArchivePath,
    pub record_trigger: RecordTrigger,
    pub kind: Option<EntryKind>,
    pub content: Option<FileContent>,
}
response_type!(AddVersion, AddVersionResponse);

#[derive(Debug, Serialize, Deserialize)]
pub struct AddVersionResponse {
    pub added: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkActionStats {
    pub affected_paths: u64,
}

/// Set the specified version as the latest one.
/// If a directory, resets all nested paths.
#[derive(Debug, Serialize, Deserialize)]
pub struct ResetVersion {
    pub path: EncryptedArchivePath,
    pub recorded_at: DateTimeUtc,
}
response_type!(ResetVersion, BulkActionStats);

/// Records rename of `old_path` to `new_path`.
/// `new_path` must not exist. If `old_path` is a directory,
/// also renames all children.
#[derive(Debug, Serialize, Deserialize)]
pub struct MovePath {
    pub old_path: EncryptedArchivePath,
    pub new_path: EncryptedArchivePath,
}
response_type!(MovePath, BulkActionStats);

/// Records deletion of the specified path.
/// If it's a directory, also records deletion of all children.
#[derive(Debug, Serialize, Deserialize)]
pub struct RemovePath {
    pub path: EncryptedArchivePath,
}
response_type!(RemovePath, BulkActionStats);

/// Checks whether the specified content hash is stored on the server.
#[derive(Debug, Serialize, Deserialize)]
pub struct ContentHashExists(pub EncryptedContentHash);
response_type!(ContentHashExists, bool);

/// Returns available space on server.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetServerStatus;
response_type!(GetServerStatus, ServerStatus);

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerStatus {
    pub available_space: u64,
}

/// Checks that file storage is consistent with database.
#[derive(Debug, Serialize, Deserialize)]
pub struct CheckIntegrity;
response_type!(CheckIntegrity, ());

/// Returns id and name of all sources.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetSources;

response_type!(GetSources, Vec<SourceInfo>);

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceInfo {
    pub id: SourceId,
    pub name: String,
}
