use {
    crate::storage::Storage,
    anyhow::{anyhow, bail, Result},
    chrono::{TimeZone, Utc},
    futures_util::{future::BoxFuture, Stream, TryStreamExt},
    rammingen_protocol::{
        endpoints::{
            AddVersion, AddVersionResponse, AddVersions, BulkActionStats, CheckIntegrity,
            ContentHashExists, GetAllEntryVersions, GetDirectChildEntries, GetEntryVersionsAtTime,
            GetNewEntries, GetServerStatus, GetSources, MovePath, RemovePath, ResetVersion,
            Response, ServerStatus, SourceInfo, StreamingResponseItem,
        },
        entry_kind_from_db, entry_kind_to_db, DateTimeUtc, EncryptedArchivePath,
        EncryptedContentHash, EncryptedSize, Entry, EntryKind, EntryVersion, EntryVersionData,
        FileContent, RecordTrigger, SourceId,
    },
    sqlx::{query, query_scalar, types::time::OffsetDateTime, PgPool, Postgres, Transaction},
    std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    },
    tokio::sync::mpsc::Sender,
};

#[derive(Debug, Clone)]
pub struct Context {
    pub db_pool: PgPool,
    pub storage: Arc<Storage>,
    pub source_id: SourceId,
}

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

macro_rules! convert_version_data {
    ($row:expr) => {{
        let row = $row;
        let kind = entry_kind_from_db(row.kind)?;
        EntryVersionData {
            path: EncryptedArchivePath::from_encrypted_without_prefix(&row.path)?,
            recorded_at: row.recorded_at.from_db(),
            source_id: row.source_id.into(),
            record_trigger: row.record_trigger.try_into()?,
            kind,
            content: if kind == Some(EntryKind::File) {
                Some(FileContent {
                    modified_at: row
                        .modified_at
                        .ok_or_else(|| anyhow!("missing modified_at for file"))?
                        .from_db(),
                    original_size: EncryptedSize::from_encrypted(
                        row.original_size
                            .ok_or_else(|| anyhow!("missing original_size for file"))?,
                    ),
                    encrypted_size: row
                        .encrypted_size
                        .ok_or_else(|| anyhow!("missing encrypted_size for file"))?
                        .try_into()?,
                    hash: EncryptedContentHash::from_encrypted(
                        row.content_hash
                            .ok_or_else(|| anyhow!("missing content_hash for file"))?
                            .into(),
                    ),
                    unix_mode: row.unix_mode.map(TryInto::try_into).transpose()?,
                })
            } else {
                None
            },
        }
    }};
}
pub(crate) use {convert_entry_version, convert_version_data};

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
            if entry.kind == EntryKind::File as i32 {
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
                    EntryKind::Directory as i32,
                    ctx.source_id.to_db(),
                    request.record_trigger as i32,
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
                EntryKind::Directory as i32
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
                    unix_mode
                ) VALUES (
                    nextval('entry_update_numbers'),
                    now(),
                    $1, $2, $3, $4, $5,
                    NULL, NULL, NULL, NULL, NULL
                ) RETURNING id",
                kind,
                parent_of_parent,
                parent.to_str_without_prefix(),
                ctx.source_id.to_db(),
                request.record_trigger as i32,
            )
            .fetch_one(&mut **tx)
            .await?
        };

        Ok(Some(entry_id))
    })
}

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
        .map(|c| i64::try_from(c.encrypted_size))
        .transpose()?;
    let modified_at_db = request
        .content
        .as_ref()
        .map(|c| c.modified_at.to_db())
        .transpose()?;
    let content_hash_db = request.content.as_ref().map(|c| c.hash.as_slice());
    if let Some(entry) = entry {
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
            .ok_or_else(|| anyhow!("missing row in response"))?;
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
            .or_else(|| entry.data.content.as_ref().and_then(|ec| ec.unix_mode))
            .map(i64::from);
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
                unix_mode = $8
            WHERE id = $9",
            ctx.source_id.to_db(),
            request.record_trigger as i32,
            entry_kind_to_db(request.kind),
            original_size_db,
            encrypted_size_db,
            modified_at_db,
            content_hash_db,
            unix_mode_db,
            entry.id.to_db(),
        )
        .execute(&mut **tx)
        .await?;
    } else {
        let unix_mode_db = request
            .content
            .as_ref()
            .and_then(|c| c.unix_mode)
            .map(i64::from);
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
                unix_mode
            ) VALUES (
                nextval('entry_update_numbers'), now(),
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10
            ) RETURNING id",
            parent,
            request.path.to_str_without_prefix(),
            ctx.source_id.to_db(),
            request.record_trigger as i32,
            entry_kind_to_db(request.kind),
            original_size_db,
            encrypted_size_db,
            modified_at_db,
            content_hash_db,
            unix_mode_db,
        )
        .fetch_one(&mut **tx)
        .await?;
    };
    Ok(AddVersionResponse { added: true })
}

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
    .ok_or_else(|| anyhow!("entry not found"))?;

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

async fn get_versions_inner<'a>(
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

pub async fn get_entry_versions_at_time(
    ctx: Context,
    request: GetEntryVersionsAtTime,
    sender: Sender<Result<StreamingResponseItem<GetEntryVersionsAtTime>>>,
) -> Result<()> {
    let mut tx = ctx.db_pool.begin().await?;
    let entries = get_versions_inner(request.recorded_at, &request.path, &mut tx).await?;
    tokio::pin!(entries);

    while let Some(entry) = entries.try_next().await? {
        if entry.data.kind.is_some() {
            sender.send(Ok(entry)).await?;
        }
    }
    Ok(())
}

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
            unix_mode = NULL
        WHERE (path = $4 OR path LIKE $5) AND kind > 0",
        ctx.source_id.to_db(),
        trigger as i32,
        EntryKind::NOT_EXISTS,
        path.to_str_without_prefix(),
        starts_with(path),
    )
    .execute(&mut **tx)
    .await?;
    Ok(r.rows_affected())
}

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
        .ok_or_else(|| anyhow!("expected 1 row in SELECT COUNT query"))?;

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

pub async fn remove_path(ctx: Context, request: RemovePath) -> Result<Response<RemovePath>> {
    let mut tx = ctx.db_pool.begin().await?;
    let affected_paths =
        remove_entries_in_dir(&ctx, &request.path, RecordTrigger::Remove, &mut tx).await?;
    tx.commit().await?;
    Ok(BulkActionStats { affected_paths })
}

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

    let entries: Vec<_> = get_versions_inner(request.recorded_at, &request.path, &mut tx)
        .await?
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
                    unix_mode = NULL
                WHERE id = $4",
                ctx.source_id.to_db(),
                RecordTrigger::Reset as i32,
                EntryKind::NOT_EXISTS,
                id,
            )
            .execute(&mut *tx)
            .await?;
            affected_paths += 1;
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
                affected_paths += 1;
            }
        }
    }
    tx.commit().await?;
    Ok(BulkActionStats { affected_paths })
}

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
                .ok_or_else(|| anyhow!("expected hash to exist in query output"))?,
        );
        let size: u64 = row
            .encrypted_size
            .ok_or_else(|| anyhow!("expected size to exist in query output"))?
            .try_into()?;
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

pub trait ToDb {
    type Output;
    fn to_db(&self) -> Self::Output;
}

impl ToDb for DateTimeUtc {
    type Output = Result<OffsetDateTime>;

    fn to_db(&self) -> Self::Output {
        const NANOS_IN_SECOND: i128 = 1_000_000_000;
        let ts_nanos = i128::from(self.timestamp()) * NANOS_IN_SECOND
            + i128::from(self.timestamp_subsec_nanos());
        OffsetDateTime::from_unix_timestamp_nanos(ts_nanos).map_err(Into::into)
    }
}

pub trait FromDb {
    type Output;
    #[allow(clippy::wrong_self_convention)]
    fn from_db(&self) -> Self::Output;
}

impl FromDb for OffsetDateTime {
    type Output = DateTimeUtc;

    fn from_db(&self) -> Self::Output {
        Utc.timestamp_nanos(self.unix_timestamp_nanos() as i64)
    }
}

pub async fn content_hash_exists(
    ctx: Context,
    request: ContentHashExists,
) -> Result<Response<ContentHashExists>> {
    ctx.storage.exists(&request.0)
}

pub async fn get_server_status(
    ctx: Context,
    _request: GetServerStatus,
) -> Result<Response<GetServerStatus>> {
    Ok(ServerStatus {
        available_space: ctx.storage.available_space()?,
    })
}
