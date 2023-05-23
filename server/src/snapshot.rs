use std::collections::HashSet;

use crate::handler::{FromDb, ToDb};
use anyhow::Result;
use chrono::Utc;
use futures_util::TryStreamExt;
use rammingen_protocol::EncryptedContentHash;
use sqlx::{query, query_scalar};
use tracing::{info, warn};

use crate::Context;

pub async fn make_snapshot(ctx: &Context) -> Result<()> {
    let mut tx = ctx.db_pool.begin().await?;

    let previous_snapshot_timestamp = if let Some(ts) =
        query_scalar!("SELECT max(timestamp) FROM snapshots")
            .fetch_one(&mut tx)
            .await?
    {
        ts
    } else if let Some(ts) = query_scalar!("SELECT min(recorded_at) FROM entry_versions")
        .fetch_one(&mut tx)
        .await?
    {
        ts
    } else {
        // There are no entries, so there is no need for a snapshot.
        return Ok(());
    };
    let next_snapshot_timestamp = previous_snapshot_timestamp.from_db()
        + chrono::Duration::from_std(ctx.config.snapshot_interval)?;
    let latest_allowed_snapshot =
        Utc::now() - chrono::Duration::from_std(ctx.config.retain_detailed_history_for)?;
    if next_snapshot_timestamp > latest_allowed_snapshot {
        return Ok(());
    }
    let next_snapshot_timestamp_db = next_snapshot_timestamp.to_db()?;

    let versions: Vec<_> = query!(
        "SELECT DISTINCT ON (path) *
        FROM entry_versions
        WHERE recorded_at <= $1 AND snapshot_id IS NULL
        ORDER BY path, recorded_at DESC",
        next_snapshot_timestamp_db,
    )
    .fetch(&mut tx)
    .map_err(anyhow::Error::from)
    .try_collect()
    .await?;
    let num_added = versions.len();

    let num_deleted = query!(
        "DELETE FROM entry_versions WHERE recorded_at <= $1 AND snapshot_id IS NULL",
        next_snapshot_timestamp_db,
    )
    .execute(&mut tx)
    .await?
    .rows_affected();

    let snapshot_id = query_scalar!(
        "INSERT INTO snapshots(timestamp) VALUES ($1) RETURNING id",
        next_snapshot_timestamp_db
    )
    .fetch_one(&mut tx)
    .await?;

    let mut hashes_to_remove = HashSet::new();
    for version in versions {
        query!("
            INSERT INTO entry_versions (
                entry_id, update_number, snapshot_id, path, recorded_at, source_id,
                record_trigger, kind, original_size, encrypted_size, modified_at, content_hash, unix_mode
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13
            );",
            version.entry_id,
            version.update_number,
            snapshot_id,
            version.path,
            next_snapshot_timestamp_db,
            version.source_id,
            version.record_trigger,
            version.kind,
            version.original_size,
            version.encrypted_size,
            version.modified_at,
            version.content_hash,
            version.unix_mode,
        ).execute(&mut tx)
        .await?;
        if let Some(hash) = version.content_hash {
            if hashes_to_remove.contains(&hash) {
                continue;
            }
            let exists = query_scalar!(
                "SELECT 1 FROM entry_versions WHERE content_hash = $1 LIMIT 1",
                hash
            )
            .fetch_optional(&mut tx)
            .await?
            .is_some();
            if !exists {
                hashes_to_remove.insert(hash);
            }
        }
    }

    tx.commit().await?;

    let mut num_removed_files = 0;
    for hash in hashes_to_remove {
        match ctx.storage.remove_file(&EncryptedContentHash(hash)) {
            Ok(()) => num_removed_files += 1,
            Err(err) => {
                warn!(?err, "failed to remove content file");
            }
        }
    }

    info!(
        "created new snapshot for {} (deleted {} versions, added {} versions, removed {} files)",
        next_snapshot_timestamp, num_deleted, num_added, num_removed_files,
    );

    Ok(())
}
