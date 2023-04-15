use std::{mem, sync::Arc};

use anyhow::Result;
use chrono::{TimeZone, Utc};
use futures_util::TryStreamExt;
use rammingen_protocol::{
    AddVersion, ArchivePath, ContentHashExists, DateTime, Entry, EntryVersion, EntryVersionData,
    FileContent, GetEntries, GetVersions, Response, SourceId, StreamingResponseItem,
};
use sqlx::{query, query_scalar, types::time::OffsetDateTime, PgPool};
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
            path: ArchivePath(row.path),
            recorded_at: row.recorded_at.from_db(),
            source_id: row.source_id.into(),
            record_trigger: row.record_trigger.try_into()?,
            kind: row.kind.try_into()?,
            exists: row.exists,
            content: if let (Some(modified_at), Some(size), Some(content_hash)) =
                (row.modified_at, row.size, row.content_hash)
            {
                Some(FileContent {
                    modified_at: modified_at.from_db(),
                    size: size.try_into()?,
                    content_hash: content_hash.into(),
                    unix_mode: row.unix_mode.map(TryInto::try_into).transpose()?,
                })
            } else {
                None
            },
        }
    }};
}

pub async fn add_version(ctx: Context, request: AddVersion) -> Result<Response<AddVersion>> {
    let mut tx = ctx.db_pool.begin().await?;
    let entry = query!("SELECT * FROM entries WHERE path = $1", request.path.0)
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
    let content_hash_db = request.content.as_ref().map(|c| &c.content_hash.0);
    let (unix_mode_db, entry_id) = if let Some(entry) = entry {
        let entry = convert_entry!(entry);
        if entry.data.is_same(&request) {
            return Ok(None);
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
                exists = $4,
                size = $5,
                modified_at = $6,
                content_hash = $7,
                unix_mode = $8
            WHERE id = $9",
            ctx.source_id.0,
            request.record_trigger as i32,
            request.kind as i32,
            request.exists,
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
        let entry_id = query_scalar!(
            "INSERT INTO entries (
                update_number,
                recorded_at,
                parent_dir,
                path,
                source_id,
                record_trigger,
                kind,
                exists,
                size,
                modified_at,
                content_hash,
                unix_mode
            ) VALUES (
                nextval('entry_update_numbers'), now(),
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10
            ) RETURNING id",
            None::<i64>, // TODO: parent dir
            request.path.0,
            ctx.source_id.0,
            request.record_trigger as i32,
            request.kind as i32,
            request.exists,
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
            exists,
            size,
            modified_at,
            content_hash,
            unix_mode
        ) VALUES (
            now(), NULL,
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10
        ) RETURNING id",
        entry_id.0,
        request.path.0,
        ctx.source_id.0,
        request.record_trigger as i32,
        request.kind as i32,
        request.exists,
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
        request.last_update_number.map(|x| x.0).unwrap_or(0)
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
            request.path.0,
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
        format!("{}/", request.path),
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
