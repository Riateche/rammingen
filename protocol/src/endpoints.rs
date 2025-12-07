use {
    crate::{
        DateTimeUtc, EncryptedContentHash, Entry, EntryKind, EntryUpdateNumber, EntryVersion,
        FileContent, RecordTrigger, SourceId, path::EncryptedArchivePath,
    },
    serde::{Deserialize, Serialize},
};

/// Trait describing a valid non-streaming request type.
pub trait RequestToResponse {
    /// Expected response type.
    type Response;
    /// URL of the endpoint that accepts this request type.
    const PATH: &'static str;
}

/// Implement `RequestToResponse` for a request type.
macro_rules! response_type {
    ($request:ty, $response:ty, $version:literal) => {
        #[allow(
            deprecated,
            clippy::allow_attributes,
            reason = "expected for deprecated requests"
        )]
        impl RequestToResponse for $request {
            type Response = $response;
            const PATH: &'static str = concat!("/api/", $version, "/", stringify!($request));
        }
    };
}

/// Trait describing a valid streaming request type.
pub trait RequestToStreamingResponse {
    /// Expected response item type.
    type ResponseItem;
    /// URL of the endpoint that accepts this request type.
    const PATH: &'static str;
}

/// Implement `RequestToStreamingResponse` for a request type.
macro_rules! streaming_response_type {
    ($request:ty, $response:ty, $version:literal) => {
        impl RequestToStreamingResponse for $request {
            type ResponseItem = $response;
            const PATH: &'static str = concat!("/api/", $version, "/", stringify!($request));
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

streaming_response_type!(GetNewEntries, Entry, "v1");

/// Returns all entries that are direct children of the specified path.
/// Results are ordered by path.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetDirectChildEntries(pub EncryptedArchivePath);

streaming_response_type!(GetDirectChildEntries, Entry, "v1");

/// Returns the version of the path corresponding to the specified time.
/// If it's a directory, also returns the version of each child path
/// at this time. Results are ordered by path.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetEntryVersionsAtTime {
    pub recorded_at: DateTimeUtc,
    pub path: EncryptedArchivePath,
}

streaming_response_type!(GetEntryVersionsAtTime, EntryVersion, "v1");

/// Returns all versions of the specified path.
/// If `recursive` is true, also returns all versions of all
/// nested paths. Results are ordered by `recorded_at`.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetAllEntryVersions {
    pub path: EncryptedArchivePath,
    pub recursive: bool,
}

streaming_response_type!(GetAllEntryVersions, EntryVersion, "v1");

/// See [`AddVersions`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddVersion {
    pub path: EncryptedArchivePath,
    pub record_trigger: RecordTrigger,
    pub kind: Option<EntryKind>,
    pub content: Option<FileContent>,
}

/// Adds a new versions of the specified paths.
///
/// If `kind` is `None`, records deletion of the path.
/// `content` must be specified only if the entry is an existing file.
/// If `unix_mode` or `is_symlink` are not specified in `content`, the previous values
/// are preserved (if any).
/// Does nothing if the specified version is considered the same
/// as the last version of this path (`record_trigger` and `modified_at`
/// do not count as meaningful changes).
#[derive(Debug, Serialize, Deserialize)]
pub struct AddVersions(pub Vec<AddVersion>);

response_type!(AddVersions, Vec<AddVersionResponse>, "v1");

/// The response to `AddVersions` will contain exactly one `AddVersionResponse`
/// per input `AddVersion`, in the same order.
#[derive(Debug, Serialize, Deserialize)]
pub struct AddVersionResponse {
    /// True if the request resulted in recording this version;
    /// false if the supplied version was the same as the current entry data.
    pub added: bool,
}

/// Return value for `ResetVersion`, `MovePath`, and `RemovePath` requests.
#[derive(Debug, Serialize, Deserialize)]
pub struct BulkActionStats {
    /// Number of paths that were changed by the request.
    pub affected_paths: u64,
}

/// Set the specified version as the latest one.
/// If a directory, resets all nested paths.
#[derive(Debug, Serialize, Deserialize)]
pub struct ResetVersion {
    pub path: EncryptedArchivePath,
    pub recorded_at: DateTimeUtc,
}
response_type!(ResetVersion, BulkActionStats, "v1");

/// Records rename of `old_path` to `new_path`.
/// `new_path` must not exist. If `old_path` is a directory,
/// also renames all children.
#[derive(Debug, Serialize, Deserialize)]
pub struct MovePath {
    pub old_path: EncryptedArchivePath,
    pub new_path: EncryptedArchivePath,
}

response_type!(MovePath, BulkActionStats, "v1");

/// Records deletion of the specified path.
/// If it's a directory, also records deletion of all children.
#[derive(Debug, Serialize, Deserialize)]
pub struct RemovePath {
    pub path: EncryptedArchivePath,
}

response_type!(RemovePath, BulkActionStats, "v1");

/// Checks whether the specified content hash is stored on the server.
#[derive(Debug, Serialize, Deserialize)]
pub struct ContentHashExists(pub EncryptedContentHash);

response_type!(ContentHashExists, bool, "v1");

/// Returns server ID and available space on server.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetServerStatus;

response_type!(GetServerStatus, ServerStatus, "v2");

/// Response to `GetServerStatus` request.
#[derive(Debug, Serialize, Deserialize)]
pub struct ServerStatus {
    /// Permanent ID of the server.
    pub server_id: String,
    /// Available space in the file storage directory in bytes.
    pub available_space: u64,
}

/// Checks that file storage is consistent with database.
///
/// The server will return an error if it finds any discrepancy.
#[derive(Debug, Serialize, Deserialize)]
pub struct CheckIntegrity;

response_type!(CheckIntegrity, (), "v1");

/// Returns ID and name of all configured sources (clients).
#[derive(Debug, Serialize, Deserialize)]
pub struct GetSources;

response_type!(GetSources, Vec<SourceInfo>, "v1");

/// Used in the response to the `GetSources` request.
#[derive(Debug, Serialize, Deserialize)]
pub struct SourceInfo {
    pub id: SourceId,
    pub name: String,
}

pub mod v1_legacy {
    use {
        super::RequestToResponse,
        serde::{Deserialize, Serialize},
    };

    /// Returns available space on server.
    #[derive(Debug, Serialize, Deserialize)]
    #[deprecated(note = "use v2 GetServerStatus instead")]
    pub struct GetServerStatus;

    response_type!(GetServerStatus, ServerStatus, "v1");

    #[derive(Debug, Serialize, Deserialize)]
    #[deprecated(note = "use v2 ServerStatus instead")]
    pub struct ServerStatus {
        pub available_space: u64,
    }
}
