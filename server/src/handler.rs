use std::{mem, sync::Arc};

use anyhow::{anyhow, bail, Result};
use chrono::{TimeZone, Utc};
use futures_util::{future::BoxFuture, TryStreamExt};
use rammingen_protocol::{
    entry_kind_from_db, entry_kind_to_db, AddVersion, ArchivePath, ContentHashExists, DateTime,
    EncryptedArchivePath, Entry, EntryKind, EntryVersion, EntryVersionData, FileContent,
    GetEntries, GetVersions, Response, SourceId, StreamingResponseItem,
};
use sqlx::{query, query_scalar, types::time::OffsetDateTime, PgPool, Postgres, Transaction};
use tokio::sync::mpsc::Sender;

use crate::storage::Storage;

const ITEMS_PER_CHUNK: usize = 1024;

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
            id: row.id.into(),
            entry_id: row.entry_id.into(),
            snapshot_id: row.snapshot_id.map(Into::into),
            data: convert_version_data!(row),
        }
    }};
}

macro_rules! convert_version_data {
    ($row:expr) => {{
        let row = $row;
        EntryVersionData {
            path: EncryptedArchivePath(ArchivePath(row.path)),
            recorded_at: row.recorded_at.from_db(),
            source_id: row.source_id.into(),
            record_trigger: row.record_trigger.try_into()?,
            kind: entry_kind_from_db(row.kind)?,
            content: if let (Some(modified_at), Some(size), Some(content_hash)) =
                (row.modified_at, row.size, row.content_hash)
            {
                Some(FileContent {
                    modified_at: modified_at.from_db(),
                    size: size.try_into()?,
                    hash: content_hash.into(),
                    unix_mode: row.unix_mode.map(TryInto::try_into).transpose()?,
                })
            } else {
                None
            },
        }
    }};
}

fn get_parent_dir<'a>(
    ctx: &'a Context,
    path: &'a EncryptedArchivePath,
    tx: &'a mut Transaction<'_, Postgres>,
    request: &'a AddVersion,
) -> BoxFuture<'a, Result<Option<i64>>> {
    Box::pin(async move {
        let Some(parent) = path.0.parent() else { return Ok(None) };
        let parent = EncryptedArchivePath(parent);
        let entry = query!("SELECT id, kind FROM entries WHERE path = $1", parent.0 .0)
            .fetch_optional(&mut *tx)
            .await?;
        let (entry_id, new_kind) = if let Some(entry) = entry {
            if entry.kind == EntryKind::File as i32 {
                bail!(
                    "cannot save entry {} because {} is a file",
                    path.0,
                    parent.0
                );
            }
            if request.kind.is_some() && entry.kind == EntryKind::NOT_EXISTS {
                // Make sure parent's parent is also marked as existing.
                let _ = get_parent_dir(ctx, &parent, &mut *tx, request).await?;

                query!(
                    "UPDATE entries SET kind = $1 WHERE id = $2",
                    EntryKind::Directory as i32,
                    entry.id
                )
                .execute(&mut *tx)
                .await?;
                (entry.id, EntryKind::Directory as i32)
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
            let id = query_scalar!(
                "INSERT INTO entries (
                    update_number,
                    recorded_at,

                    kind,
                    parent_dir,
                    path,
                    source_id,
                    record_trigger,

                    size,
                    modified_at,
                    content_hash,
                    unix_mode
                ) VALUES (
                    nextval('entry_update_numbers'),
                    now(),
                    $1, $2, $3, $4, $5,
                    NULL, NULL, NULL, NULL
                ) RETURNING id",
                kind,
                parent_of_parent,
                parent.0 .0,
                ctx.source_id.0,
                request.record_trigger as i32,
            )
            .fetch_one(&mut *tx)
            .await?;
            (id, kind)
        };

        query_scalar!(
            "INSERT INTO entry_versions (
                recorded_at,
                snapshot_id,

                kind,
                entry_id,
                path,
                source_id,
                record_trigger,

                size,
                modified_at,
                content_hash,
                unix_mode
            ) VALUES (
                now(), NULL,
                $1, $2, $3, $4, $5,
                NULL, NULL, NULL, NULL
            )",
            new_kind,
            entry_id,
            request.path.0 .0,
            ctx.source_id.0,
            request.record_trigger as i32,
        )
        .execute(&mut *tx)
        .await?;
        Ok(Some(entry_id))
    })
}

pub async fn add_version(ctx: Context, request: AddVersion) -> Result<Response<AddVersion>> {
    let mut tx = ctx.db_pool.begin().await?;
    let entry = query!("SELECT * FROM entries WHERE path = $1", request.path.0 .0)
        .fetch_optional(&mut tx)
        .await?;
    let size_db = request
        .content
        .as_ref()
        .map(|c| i64::try_from(c.size))
        .transpose()?;
    let modified_at_db = request
        .content
        .as_ref()
        .map(|c| c.modified_at.to_db())
        .transpose()?;
    let content_hash_db = request.content.as_ref().map(|c| &c.hash.0);
    let (unix_mode_db, entry_id) = if let Some(entry) = entry {
        let entry = convert_entry!(entry);
        if entry.data.is_same(&request) {
            return Ok(None);
        }
        if request.kind.is_none() {
            let child_count = query_scalar!(
                "SELECT count(*) FROM entries
                WHERE kind != 0 AND parent_dir = $1",
                Some(entry.id.0)
            )
            .fetch_one(&mut tx)
            .await?
            .ok_or_else(|| anyhow!("missing row in response"))?;
            if child_count > 0 {
                bail!(
                    "cannot mark {} as deleted because it has existing children",
                    request.path.0
                );
            }
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
                size = $4,
                modified_at = $5,
                content_hash = $6,
                unix_mode = $7
            WHERE id = $8",
            ctx.source_id.0,
            request.record_trigger as i32,
            entry_kind_to_db(request.kind),
            size_db,
            modified_at_db,
            content_hash_db,
            unix_mode_db,
            entry.id.0,
        )
        .execute(&mut tx)
        .await?;
        (unix_mode_db, entry.id)
    } else {
        let unix_mode_db = request
            .content
            .as_ref()
            .and_then(|c| c.unix_mode)
            .map(i64::from);
        let parent = get_parent_dir(&ctx, &request.path, &mut tx, &request).await?;
        let entry_id = query_scalar!(
            "INSERT INTO entries (
                update_number,
                recorded_at,
                parent_dir,
                path,
                source_id,
                record_trigger,
                kind,
                size,
                modified_at,
                content_hash,
                unix_mode
            ) VALUES (
                nextval('entry_update_numbers'), now(),
                $1, $2, $3, $4, $5, $6, $7, $8, $9
            ) RETURNING id",
            parent,
            request.path.0 .0,
            ctx.source_id.0,
            request.record_trigger as i32,
            entry_kind_to_db(request.kind),
            size_db,
            modified_at_db,
            content_hash_db,
            unix_mode_db,
        )
        .fetch_one(&mut tx)
        .await?;
        (unix_mode_db, entry_id.into())
    };

    let version_id = query_scalar!(
        "INSERT INTO entry_versions (
            recorded_at,
            snapshot_id,
            entry_id,
            path,
            source_id,
            record_trigger,
            kind,
            size,
            modified_at,
            content_hash,
            unix_mode
        ) VALUES (
            now(), NULL,
            $1, $2, $3, $4, $5, $6, $7, $8, $9
        ) RETURNING id",
        entry_id.0,
        request.path.0 .0,
        ctx.source_id.0,
        request.record_trigger as i32,
        entry_kind_to_db(request.kind),
        size_db,
        modified_at_db,
        content_hash_db,
        unix_mode_db,
    )
    .fetch_one(&mut tx)
    .await?;

    tx.commit().await?;
    Ok(Some(version_id.into()))
}

pub async fn get_entries(
    ctx: Context,
    request: GetEntries,
    tx: Sender<Result<Option<StreamingResponseItem<GetEntries>>>>,
) -> Result<()> {
    let mut output = Vec::new();
    let mut rows = query!(
        "SELECT * FROM entries WHERE update_number > $1",
        request.last_update_number.0
    )
    .fetch(&ctx.db_pool);
    while let Some(row) = rows.try_next().await? {
        output.push(convert_entry!(row));
        if output.len() >= ITEMS_PER_CHUNK {
            tx.send(Ok(Some(mem::take(&mut output)))).await?;
        }
    }
    tx.send(Ok(Some(output))).await?;
    Ok(())
}

pub async fn get_versions(
    ctx: Context,
    request: GetVersions,
    tx: Sender<Result<Option<StreamingResponseItem<GetVersions>>>>,
) -> Result<()> {
    let mut output = Vec::new();
    let mut rows =
        query!(
            "SELECT * FROM entry_versions WHERE path = $1 AND recorded_at <= $2 ORDER BY recorded_at DESC LIMIT 1",
            request.path.0 .0,
            request.recorded_at.to_db()?,
        ).fetch(&ctx.db_pool);

    while let Some(row) = rows.try_next().await? {
        output.push(convert_entry_version!(row));
        if output.len() >= ITEMS_PER_CHUNK {
            tx.send(Ok(Some(mem::take(&mut output)))).await?;
        }
    }

    let mut rows2 = query!(
        "SELECT * FROM (
            SELECT *,
            row_number() OVER(PARTITION BY entry_id ORDER BY recorded_at DESC) AS row_number
            FROM entry_versions
            WHERE path LIKE $1 AND recorded_at <= $2
        ) t WHERE row_number = 1",
        format!("{}/", request.path.0),
        request.recorded_at.to_db()?,
    )
    .fetch(&ctx.db_pool);

    while let Some(row) = rows2.try_next().await? {
        output.push(convert_entry_version!(row));
        if output.len() >= ITEMS_PER_CHUNK {
            tx.send(Ok(Some(mem::take(&mut output)))).await?;
        }
    }

    tx.send(Ok(Some(output))).await?;
    Ok(())
}

trait ToDb {
    type Output;
    fn to_db(&self) -> Self::Output;
}

impl ToDb for DateTime {
    type Output = Result<OffsetDateTime>;

    fn to_db(&self) -> Self::Output {
        Ok(OffsetDateTime::from_unix_timestamp_nanos(
            self.timestamp_nanos().into(),
        )?)
    }
}

trait FromDb {
    type Output;
    #[allow(clippy::wrong_self_convention)]
    fn from_db(&self) -> Self::Output;
}

impl FromDb for OffsetDateTime {
    type Output = DateTime;

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
