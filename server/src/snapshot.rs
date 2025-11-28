use {
    crate::{handler::DateTimeUtcExt, Context},
    anyhow::{Context as _, Result},
    chrono::Utc,
    futures::TryStreamExt,
    rammingen_protocol::{DateTimeUtc, EncryptedContentHash},
    sqlx::{query, query_scalar},
    std::collections::HashSet,
    tracing::{info, warn},
};

/// Create a new snapshot if the configuration and current time allows.
/// Delete older entry versions that are not part of a snapshot.
pub async fn make_snapshot(ctx: &Context) -> Result<()> {
    let mut tx = ctx.db_pool.begin().await?;

    let previous_snapshot_timestamp = if let Some(ts) =
        query_scalar!("SELECT max(timestamp) FROM snapshots")
            .fetch_one(&mut *tx)
            .await?
    {
        ts
    } else if let Some(ts) = query_scalar!("SELECT min(recorded_at) FROM entry_versions")
        .fetch_one(&mut *tx)
        .await?
    {
        ts
    } else {
        // There are no entries, so there is no need for a snapshot.
        return Ok(());
    };
    let next_snapshot_timestamp = DateTimeUtc::from_db(previous_snapshot_timestamp)?
        .checked_add_signed(chrono::Duration::from_std(ctx.config.snapshot_interval)?)
        .context("next_snapshot_timestamp overflow")?;
    let latest_allowed_snapshot = Utc::now()
        .checked_sub_signed(chrono::Duration::from_std(
            ctx.config.retain_detailed_history_for,
        )?)
        .context("latest_allowed_snapshot underflow")?;
    if next_snapshot_timestamp > latest_allowed_snapshot {
        return Ok(());
    }
    let next_snapshot_timestamp_db = next_snapshot_timestamp.to_db()?;

    // Get latest version at or before the new snapshot's timestamp
    // for every entry. Excludes entries that haven't changed since the last snapshot.
    let versions: Vec<_> = query!(
        "SELECT DISTINCT ON (path) *
        FROM entry_versions
        WHERE recorded_at <= $1 AND snapshot_id IS NULL
        ORDER BY path, recorded_at DESC",
        next_snapshot_timestamp_db,
    )
    .fetch(&mut *tx)
    .map_err(anyhow::Error::from)
    .try_collect()
    .await?;
    let num_added = versions.len();

    let mut hashes_to_check = HashSet::new();
    let mut num_deleted = 0;
    {
        // Delete all non-snapshot entry versions at or before the new snapshot's timestamp.
        let mut deleted_rows = query_scalar!(
            "DELETE FROM entry_versions
            WHERE recorded_at <= $1 AND snapshot_id IS NULL
            RETURNING content_hash",
            next_snapshot_timestamp_db,
        )
        .fetch(&mut *tx);
        while let Some(hash) = deleted_rows.try_next().await? {
            num_deleted += 1;
            if let Some(hash) = hash {
                hashes_to_check.insert(EncryptedContentHash::from_encrypted(hash));
            }
        }
    }

    // Add a snapshot record.
    let snapshot_id = query_scalar!(
        "INSERT INTO snapshots(timestamp) VALUES ($1) RETURNING id",
        next_snapshot_timestamp_db
    )
    .fetch_one(&mut *tx)
    .await?;

    let mut hashes_to_remove = Vec::new();
    for version in versions {
        // Insert all previously found entry versions as new versions linked to the new snapshot.
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
        ).execute(&mut *tx)
        .await?;
        if let Some(hash) = version.content_hash {
            hashes_to_check.insert(EncryptedContentHash::from_encrypted(hash));
        }
    }
    // For every hash that was affected by previous modifications,
    // check if there are any remaining entry versions that use that hash.
    for hash in hashes_to_check {
        let exists = query_scalar!(
            "SELECT 1 FROM entry_versions WHERE content_hash = $1 LIMIT 1",
            hash.as_slice()
        )
        .fetch_optional(&mut *tx)
        .await?
        .is_some();
        if !exists {
            hashes_to_remove.push(hash);
        }
    }

    tx.commit().await?;

    // Remove files that are no longer referenced by any entry versions.
    let mut num_removed_files = 0;
    for hash in hashes_to_remove {
        match ctx.storage.remove_file(&hash) {
            Ok(()) => num_removed_files += 1,
            Err(err) => {
                warn!(?err, "failed to remove content file");
            }
        }
    }

    info!(
        "Created new snapshot for {} (deleted {} versions, added {} versions, removed {} files)",
        next_snapshot_timestamp, num_deleted, num_added, num_removed_files,
    );

    Ok(())
}
