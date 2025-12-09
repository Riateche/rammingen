use {
    crate::storage::Storage,
    anyhow::{Context as _, Result, bail},
    cadd::{
        ops::{Cadd, Cmul},
        prelude::IntoType,
    },
    chrono::{TimeZone, Utc},
    futures::{Stream, TryStreamExt, future::BoxFuture},
    rammingen_protocol::{
        DateTimeUtc, EncryptedArchivePath, EncryptedContentHash, EncryptedSize, Entry, EntryKind,
        EntryVersion, EntryVersionData, FileContent, RecordTrigger, SourceId,
        endpoints::{
            AddVersion, AddVersionResponse, AddVersions, BulkActionStats, CheckIntegrity,
            ContentHashExists, GetAllEntryVersions, GetDirectChildEntries, GetEntryVersionsAtTime,
            GetNewEntries, GetServerStatus, GetSources, MovePath, RemovePath, ResetVersion,
            Response, ServerStatus, SourceInfo, StreamingResponseItem, v1_legacy,
        },
        entry_kind_from_db, entry_kind_to_db,
    },
    sqlx::{PgPool, Postgres, Transaction, query, query_scalar, types::time::OffsetDateTime},
    std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    },
    tokio::sync::mpsc::Sender,
};

/// Context for handling requests from a client.
#[derive(Debug, Clone)]
pub struct Context {
    pub db_pool: PgPool,
    pub server_id: Arc<str>,
    pub storage: Arc<Storage>,
    /// ID of the connected client.
    pub source_id: SourceId,
}

/// Convert a database row into an `Entry`.
///
/// `sqlx` macros generate a separate row type for every query, even if the fields are the same.
/// These conversion macros are the easiest way to convert all of them to our internal types.
macro_rules! convert_entry {
    ($row:expr) => {{
        let row = $row;
        Entry {
            id: row.id.into(),
            update_number: row.update_number.into(),
            parent_dir: row.parent_dir.map(Into::into),
            data: convert_version_data!(row),
        }
    }};
}

/// Convert a database row into an `EntryVersion`.
macro_rules! convert_entry_version {
    ($row:expr) => {{
        let row = $row;
        EntryVersion {
            entry_id: row.entry_id.into(),
            snapshot_id: row.snapshot_id.map(Into::into),
            data: convert_version_data!(row),
        }
    }};
}

/// Convert a database row into an `EntryVersionData`.
macro_rules! convert_version_data {
    ($row:expr) => {{
        let row = $row;
        let kind = entry_kind_from_db(row.kind)?;
        EntryVersionData {
            path: EncryptedArchivePath::from_encrypted_without_prefix(&row.path)?,
            recorded_at: DateTimeUtc::from_db(row.recorded_at)?,
            source_id: row.source_id.into(),
            record_trigger: RecordTrigger::from_db(row.record_trigger)?,
            kind,
            content: if kind == Some(EntryKind::File) {
                Some(FileContent {
                    modified_at: DateTimeUtc::from_db(
                        row.modified_at.context("missing modified_at for file")?,
                    )?,
                    original_size: EncryptedSize::from_encrypted(
                        row.original_size
                            .context("missing original_size for file")?,
                    ),
                    encrypted_size: row
                        .encrypted_size
                        .context("missing encrypted_size for file")?
                        .try_into()?,
                    hash: EncryptedContentHash::from_encrypted(
                        row.content_hash
                            .context("missing content_hash for file")?
                            .into(),
                    ),
                    unix_mode: row.unix_mode.map(TryInto::try_into).transpose()?,
                    is_symlink: row.is_symlink,
                })
            } else {
                None
            },
        }
    }};
}
pub(crate) use {convert_entry_version, convert_version_data};

/// Find or create a database entry corresponding to the parent path of `path`.
///
/// Returns the ID of the entry, or `None` if `path` is the root path.
fn get_parent_dir<'a>(
    ctx: &'a Context,
    path: &'a EncryptedArchivePath,
    tx: &'a mut Transaction<'_, Postgres>,
    request: &'a AddVersion,
) -> BoxFuture<'a, Result<Option<i64>>> {
    Box::pin(async move {
        let Some(parent) = path.parent() else {
            return Ok(None);
        };
        let entry = query!(
            "SELECT id, kind FROM entries WHERE path = $1",
            parent.to_str_without_prefix()
        )
        .fetch_optional(&mut **tx)
        .await?;
        let entry_id = if let Some(entry) = entry {
            if entry.kind == EntryKind::File.to_db() {
                bail!("cannot save entry {} because {} is a file", path, parent);
            }
            if request.kind.is_some() && entry.kind == EntryKind::NOT_EXISTS {
                // Make sure parent's parent is also marked as existing.
                let _ = get_parent_dir(ctx, &parent, &mut *tx, request).await?;

                query!(
                    "UPDATE entries SET
                        update_number = nextval('entry_update_numbers'),
                        recorded_at = now(),
                        kind = $1,
                        source_id = $2,
                        record_trigger = $3
                    WHERE id = $4",
                    EntryKind::Directory.to_db(),
                    ctx.source_id.to_db(),
                    request.record_trigger.to_db(),
                    entry.id,
                )
                .execute(&mut **tx)
                .await?;
                entry.id
            } else {
                return Ok(Some(entry.id));
            }
        } else {
            let parent_of_parent = get_parent_dir(ctx, &parent, &mut *tx, request).await?;
            let kind = if request.kind.is_some() {
                EntryKind::Directory.to_db()
            } else {
                EntryKind::NOT_EXISTS
            };
            query_scalar!(
                "INSERT INTO entries (
                    update_number,
                    recorded_at,

                    kind,
                    parent_dir,
                    path,
                    source_id,
                    record_trigger,

                    original_size,
                    encrypted_size,
                    modified_at,
                    content_hash,
                    unix_mode,
                    is_symlink
                ) VALUES (
                    nextval('entry_update_numbers'),
                    now(),
                    $1, $2, $3, $4, $5,
                    NULL, NULL, NULL, NULL, NULL, NULL
                ) RETURNING id",
                kind,
                parent_of_parent,
                parent.to_str_without_prefix(),
                ctx.source_id.to_db(),
                request.record_trigger.to_db(),
            )
            .fetch_one(&mut **tx)
            .await?
        };

        Ok(Some(entry_id))
    })
}

/// Create or update an entry in the database with data from `request`.
async fn add_version_inner<'a>(
    ctx: &'a Context,
    request: AddVersion,
    tx: &'a mut Transaction<'_, Postgres>,
) -> Result<AddVersionResponse> {
    if let Some(content) = &request.content {
        if !ctx.storage.exists(&content.hash)? {
            bail!("cannot add version: hash not found in storage");
        }
        let storage_size = ctx.storage.file_size(&content.hash)?;
        if content.encrypted_size != storage_size {
            bail!(
                "cannot add version: size mismatch: {} in request, {} in storage",
                content.encrypted_size,
                storage_size
            );
        }
    }
    let entry = query!(
        "SELECT * FROM entries WHERE path = $1",
        request.path.to_str_without_prefix()
    )
    .fetch_optional(&mut **tx)
    .await?;
    let original_size_db = request.content.as_ref().map(|c| c.original_size.as_slice());
    let encrypted_size_db = request
        .content
        .as_ref()
        .map(|c| c.encrypted_size.try_into_type::<i64>())
        .transpose()?;
    let modified_at_db = request
        .content
        .as_ref()
        .map(|c| c.modified_at.to_db())
        .transpose()?;
    let content_hash_db = request.content.as_ref().map(|c| c.hash.as_slice());
    if let Some(entry) = entry {
        // Updating an existing entry.
        let entry = convert_entry!(entry);
        if entry.data.is_same(&request) {
            return Ok(AddVersionResponse { added: false });
        }
        if request.kind.is_none() {
            let child_count = query_scalar!(
                "SELECT count(*) FROM entries
                WHERE kind != 0 AND parent_dir = $1",
                entry.id.to_db()
            )
            .fetch_one(&mut **tx)
            .await?
            .context("missing row in response")?;
            if child_count > 0 {
                bail!(
                    "cannot mark {} as deleted because it has existing children (request: {:?})",
                    request.path,
                    request,
                );
            }
        }
        if request.kind.is_some() && entry.data.kind.is_none() {
            // Make sure parent is marked as existing.
            let _ = get_parent_dir(ctx, &request.path, &mut *tx, &request).await?;
        }
        let unix_mode_db = request
            .content
            .as_ref()
            .and_then(|c| c.unix_mode)
            .or_else(|| entry.data.content.as_ref()?.unix_mode)
            .map(i64::from);
        let is_symlink_db = request
            .content
            .as_ref()
            .and_then(|c| c.is_symlink)
            .or_else(|| entry.data.content.as_ref()?.is_symlink);
        query!(
            "UPDATE entries
            SET update_number = nextval('entry_update_numbers'),
                recorded_at = now(),
                source_id = $1,
                record_trigger = $2,
                kind = $3,
                original_size = $4,
                encrypted_size = $5,
                modified_at = $6,
                content_hash = $7,
                unix_mode = $8,
                is_symlink = $9
            WHERE id = $10",
            ctx.source_id.to_db(),
            request.record_trigger.to_db(),
            entry_kind_to_db(request.kind),
            original_size_db,
            encrypted_size_db,
            modified_at_db,
            content_hash_db,
            unix_mode_db,
            is_symlink_db,
            entry.id.to_db(),
        )
        .execute(&mut **tx)
        .await?;
    } else {
        // Creating a new entry.
        let unix_mode_db = request
            .content
            .as_ref()
            .and_then(|c| c.unix_mode)
            .map(i64::from);
        let is_symlink_db = request.content.as_ref().and_then(|c| c.is_symlink);
        let parent = get_parent_dir(ctx, &request.path, &mut *tx, &request).await?;
        query_scalar!(
            "INSERT INTO entries (
                update_number,
                recorded_at,
                parent_dir,
                path,
                source_id,
                record_trigger,
                kind,
                original_size,
                encrypted_size,
                modified_at,
                content_hash,
                unix_mode,
                is_symlink
            ) VALUES (
                nextval('entry_update_numbers'), now(),
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11
            ) RETURNING id",
            parent,
            request.path.to_str_without_prefix(),
            ctx.source_id.to_db(),
            request.record_trigger.to_db(),
            entry_kind_to_db(request.kind),
            original_size_db,
            encrypted_size_db,
            modified_at_db,
            content_hash_db,
            unix_mode_db,
            is_symlink_db,
        )
        .fetch_one(&mut **tx)
        .await?;
    }
    Ok(AddVersionResponse { added: true })
}

/// Add multiple entry versions in a single transaction.
pub async fn add_versions(ctx: Context, request: AddVersions) -> Result<Response<AddVersions>> {
    let mut tx = ctx.db_pool.begin().await?;
    let mut results = Vec::new();
    for item in request.0 {
        let r = add_version_inner(&ctx, item, &mut tx).await?;
        results.push(r);
    }
    tx.commit().await?;
    Ok(results)
}

/// Get entries added or updated since the specified update number.
pub async fn get_new_entries(
    ctx: Context,
    request: GetNewEntries,
    tx: Sender<Result<StreamingResponseItem<GetNewEntries>>>,
) -> Result<()> {
    let mut rows = query!(
        "SELECT * FROM entries WHERE update_number > $1 ORDER BY update_number",
        request.last_update_number.to_db()
    )
    .fetch(&ctx.db_pool);
    while let Some(row) = rows.try_next().await? {
        tx.send(Ok(convert_entry!(row))).await?;
    }
    Ok(())
}

/// Get content of a directory.
pub async fn get_direct_child_entries(
    ctx: Context,
    request: GetDirectChildEntries,
    tx: Sender<Result<StreamingResponseItem<GetDirectChildEntries>>>,
) -> Result<()> {
    let main_entry_id = query_scalar!(
        "SELECT id FROM entries WHERE path = $1",
        request.0.to_str_without_prefix()
    )
    .fetch_optional(&ctx.db_pool)
    .await?
    .context("entry not found")?;

    let mut rows = query!(
        "SELECT * FROM entries WHERE parent_dir = $1 ORDER BY path",
        main_entry_id
    )
    .fetch(&ctx.db_pool);
    while let Some(row) = rows.try_next().await? {
        tx.send(Ok(convert_entry!(row))).await?;
    }
    Ok(())
}

/// Get the last version at or before `recorded_at` of the entry for `path` and
/// all entries for direct or indirect subdirectories of `path`.
fn get_versions_inner<'a>(
    recorded_at: DateTimeUtc,
    path: &'a EncryptedArchivePath,
    tx: &'a mut Transaction<'_, Postgres>,
) -> Result<impl Stream<Item = Result<EntryVersion>> + 'a> {
    let stream = query!(
        "SELECT DISTINCT ON (path) *
        FROM entry_versions
        WHERE (path = $1 OR path LIKE $2) AND recorded_at <= $3
        ORDER BY path, recorded_at DESC",
        path.to_str_without_prefix(),
        starts_with(&path),
        recorded_at.to_db()?,
    )
    .fetch(&mut **tx)
    .map_err(anyhow::Error::from)
    .and_then(|row| async move { Ok(convert_entry_version!(row)) });
    Ok(stream)
}

/// Get the last version at or before `request.recorded_at` of the entry for `request.path` and
/// all entries for direct or indirect subdirectories of `request.path`.
pub async fn get_entry_versions_at_time(
    ctx: Context,
    request: GetEntryVersionsAtTime,
    sender: Sender<Result<StreamingResponseItem<GetEntryVersionsAtTime>>>,
) -> Result<()> {
    let mut tx = ctx.db_pool.begin().await?;
    let entries = get_versions_inner(request.recorded_at, &request.path, &mut tx)?;
    tokio::pin!(entries);

    while let Some(entry) = entries.try_next().await? {
        if entry.data.kind.is_some() {
            sender.send(Ok(entry)).await?;
        }
    }
    Ok(())
}

/// Get all versions of an entry for `request.path`. If `request.recursive` is `true`,
/// also get all versions of all entries for direct or indirect subdirectories of `request.path`.
pub async fn get_all_entry_versions(
    ctx: Context,
    request: GetAllEntryVersions,
    tx: Sender<Result<StreamingResponseItem<GetAllEntryVersions>>>,
) -> Result<()> {
    if request.recursive {
        let mut rows = query!(
            "SELECT * FROM entry_versions
            WHERE path = $1 OR path LIKE $2
            ORDER BY id",
            request.path.to_str_without_prefix(),
            starts_with(&request.path)
        )
        .fetch(&ctx.db_pool);
        while let Some(row) = rows.try_next().await? {
            tx.send(Ok(convert_entry_version!(row))).await?;
        }
    } else {
        let mut rows = query!(
            "SELECT * FROM entry_versions WHERE path = $1 ORDER BY id",
            request.path.to_str_without_prefix()
        )
        .fetch(&ctx.db_pool);
        while let Some(row) = rows.try_next().await? {
            tx.send(Ok(convert_entry_version!(row))).await?;
        }
    }
    Ok(())
}

/// Returns a SQL LIKE pattern that matches any path inside `path`, excluding the path itself.
fn starts_with(path: &EncryptedArchivePath) -> String {
    if path.to_str_without_prefix() == "/" {
        "/%".into()
    } else {
        format!(
            "{}/%",
            path.to_str_without_prefix()
                .replace('\\', r"\\")
                .replace('%', r"\%")
                .replace('_', r"\_")
        )
    }
}

/// Marks entries for `path` and all its content as deleted.
async fn remove_entries_in_dir<'a>(
    ctx: &'a Context,
    path: &'a EncryptedArchivePath,
    trigger: RecordTrigger,
    tx: &'a mut Transaction<'_, Postgres>,
) -> Result<u64> {
    let r = query!(
        "UPDATE entries
        SET update_number = nextval('entry_update_numbers'),
            recorded_at = now(),
            source_id = $1,
            record_trigger = $2,
            kind = $3,
            original_size = NULL,
            encrypted_size = NULL,
            modified_at = NULL,
            content_hash = NULL,
            unix_mode = NULL,
            is_symlink = NULL
        WHERE (path = $4 OR path LIKE $5) AND kind > 0",
        ctx.source_id.to_db(),
        trigger.to_db(),
        EntryKind::NOT_EXISTS,
        path.to_str_without_prefix(),
        starts_with(path),
    )
    .execute(&mut **tx)
    .await?;
    Ok(r.rows_affected())
}

/// Copy all entries at `request.old_path` (including subdirectories) to `request.new_path`.
/// Mark all entries at `request.old_path` as deleted.
pub async fn move_path(ctx: Context, request: MovePath) -> Result<Response<MovePath>> {
    let mut tx = ctx.db_pool.begin().await?;
    let mut old_entries = Vec::new();
    {
        let count_existing = query_scalar!(
            "SELECT COUNT(*) FROM entries WHERE (path = $1 OR path LIKE $2) AND kind > 0",
            request.new_path.to_str_without_prefix(),
            starts_with(&request.new_path)
        )
        .fetch_one(&mut *tx)
        .await?
        .context("expected 1 row in SELECT COUNT query")?;

        if count_existing > 0 {
            bail!("destination path already exists");
        }

        let mut entries = query!(
            "SELECT * FROM entries WHERE (path = $1 OR path LIKE $2) AND kind > 0 ORDER BY path",
            request.old_path.to_str_without_prefix(),
            starts_with(&request.old_path),
        )
        .fetch(&mut *tx);
        while let Some(row) = entries.try_next().await? {
            old_entries.push(convert_entry!(row));
        }
    }

    remove_entries_in_dir(&ctx, &request.old_path, RecordTrigger::Move, &mut tx).await?;

    let affected_paths = old_entries.len().try_into()?;
    for entry in old_entries {
        let new_path = if entry.data.path == request.old_path {
            request.new_path.clone()
        } else if let Some(relative) = entry.data.path.strip_prefix(&request.old_path) {
            request.new_path.join_multiple(relative)?
        } else {
            bail!("strip_prefix failed while processing entry: {:?}", entry);
        };
        let add_version = AddVersion {
            path: new_path,
            record_trigger: RecordTrigger::Move,
            kind: entry.data.kind,
            content: entry.data.content,
        };
        let result = add_version_inner(&ctx, add_version, &mut tx).await?;
        if !result.added {
            bail!("unexpected added = false while moving path");
        }
    }

    tx.commit().await?;
    Ok(BulkActionStats { affected_paths })
}

/// Marks entries for `path` and all its content as deleted.
pub async fn remove_path(ctx: Context, request: RemovePath) -> Result<Response<RemovePath>> {
    let mut tx = ctx.db_pool.begin().await?;
    let affected_paths =
        remove_entries_in_dir(&ctx, &request.path, RecordTrigger::Remove, &mut tx).await?;
    tx.commit().await?;
    Ok(BulkActionStats { affected_paths })
}

/// Restore `request.path` (including subdirectories) to the latest state recorded at or before `request.recorded_at`.
pub async fn reset_version(ctx: Context, request: ResetVersion) -> Result<Response<ResetVersion>> {
    let mut tx = ctx.db_pool.begin().await?;

    let old_existing_ids = query_scalar!(
        "SELECT id FROM entries
        WHERE (path = $1 OR path LIKE $2) AND kind > 0
        ORDER BY path DESC",
        request.path.to_str_without_prefix(),
        starts_with(&request.path),
    )
    .fetch_all(&mut *tx)
    .await?;

    let entries: Vec<_> = get_versions_inner(request.recorded_at, &request.path, &mut tx)?
        .try_collect()
        .await?;
    let new_existing_ids: HashSet<i64> = entries
        .iter()
        .filter(|entry| entry.data.kind.is_some())
        .map(|entry| entry.entry_id.into())
        .collect();
    let mut affected_paths = 0;

    for id in old_existing_ids {
        if !new_existing_ids.contains(&id) {
            tracing::debug!("reset_version: deleting {:?}", id);
            query!(
                "UPDATE entries
                SET update_number = nextval('entry_update_numbers'),
                    recorded_at = now(),
                    source_id = $1,
                    record_trigger = $2,
                    kind = $3,
                    original_size = NULL,
                    encrypted_size = NULL,
                    modified_at = NULL,
                    content_hash = NULL,
                    unix_mode = NULL,
                    is_symlink = NULL
                WHERE id = $4",
                ctx.source_id.to_db(),
                RecordTrigger::Reset.to_db(),
                EntryKind::NOT_EXISTS,
                id,
            )
            .execute(&mut *tx)
            .await?;
            affected_paths = affected_paths.cadd(1u64)?;
        }
    }

    for entry in entries {
        if entry.data.kind.is_some() {
            let r = add_version_inner(
                &ctx,
                AddVersion {
                    path: entry.data.path,
                    record_trigger: RecordTrigger::Reset,
                    kind: entry.data.kind,
                    content: entry.data.content,
                },
                &mut tx,
            )
            .await?;
            if r.added {
                affected_paths = affected_paths.cadd(1u64)?;
            }
        }
    }
    tx.commit().await?;
    Ok(BulkActionStats { affected_paths })
}

/// Verifies storage invariants:
/// - for any recorded content hash, there is a corresponding content file in file storage with the matching size;
/// - for any file in file storage, there is a corresponding content hash in the database.
pub async fn check_integrity(
    ctx: Context,
    _request: CheckIntegrity,
) -> Result<Response<CheckIntegrity>> {
    let mut db_hashes = HashMap::new();
    let mut rows = query!(
        "SELECT encrypted_size, content_hash FROM entry_versions WHERE content_hash IS NOT NULL"
    )
    .fetch(&ctx.db_pool);
    while let Some(row) = rows.try_next().await? {
        let hash = EncryptedContentHash::from_encrypted(
            row.content_hash
                .context("expected hash to exist in query output")?,
        );
        let size = row
            .encrypted_size
            .context("expected size to exist in query output")?
            .try_into_type::<u64>()?;
        db_hashes.insert(hash, size);
    }

    let storage_hashes = ctx.storage.all_hashes_and_sizes()?;
    for (hash, size) in &db_hashes {
        if let Some(size2) = storage_hashes.get(hash) {
            if size != size2 {
                bail!(
                    "size mismatch for hash {}: {} in db, {} in storage",
                    hash.to_url_safe(),
                    size,
                    size2
                );
            }
        } else {
            bail!("hash not found in storage: {}", hash.to_url_safe());
        }
    }
    for hash in storage_hashes.keys() {
        if !db_hashes.contains_key(hash) {
            bail!("hash not found in db: {}", hash.to_url_safe());
        }
    }

    Ok(())
}

/// Get ID and name of configured sources (clients).
pub async fn get_sources(ctx: Context, _request: GetSources) -> Result<Response<GetSources>> {
    let mut sources = Vec::new();
    let mut rows = query!("SELECT id, name FROM sources ORDER BY id").fetch(&ctx.db_pool);
    while let Some(row) = rows.try_next().await? {
        sources.push(SourceInfo {
            id: row.id.into(),
            name: row.name,
        });
    }

    Ok(sources)
}

pub trait DateTimeUtcExt: Sized {
    fn to_db(&self) -> Result<OffsetDateTime>;
    fn from_db(value: OffsetDateTime) -> Result<Self>;
}

impl DateTimeUtcExt for DateTimeUtc {
    fn to_db(&self) -> Result<OffsetDateTime> {
        const NANOS_IN_SECOND: i128 = 1_000_000_000;
        let ts_nanos = self
            .timestamp()
            .into_type::<i128>()
            .cmul(NANOS_IN_SECOND)?
            .cadd(self.timestamp_subsec_nanos().into_type::<i128>())?;
        OffsetDateTime::from_unix_timestamp_nanos(ts_nanos).map_err(Into::into)
    }

    fn from_db(value: OffsetDateTime) -> Result<Self> {
        let ts_nanos = value.unix_timestamp_nanos().try_into()?;
        Ok(Utc.timestamp_nanos(ts_nanos))
    }
}

/// Check if content hash exists in file storage.
pub async fn content_hash_exists(
    ctx: Context,
    request: ContentHashExists,
) -> Result<Response<ContentHashExists>> {
    ctx.storage.exists(&request.0)
}

/// Get available space on the server.
#[expect(deprecated, reason = "handling deprecated request")]
pub async fn get_server_status_v1_legacy(
    ctx: Context,
    _request: v1_legacy::GetServerStatus,
) -> Result<Response<v1_legacy::GetServerStatus>> {
    Ok(v1_legacy::ServerStatus {
        available_space: ctx.storage.available_space()?,
    })
}

/// Get available space on the server.
pub async fn get_server_status(
    ctx: Context,
    _request: GetServerStatus,
) -> Result<Response<GetServerStatus>> {
    Ok(ServerStatus {
        server_id: ctx.server_id.to_string(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        available_space: ctx.storage.available_space()?,
    })
}
