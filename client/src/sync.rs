use std::{collections::HashSet, sync::Arc, time::Duration};

use crate::{
    counters::NotificationCounters,
    download::download_latest,
    pull_updates::pull_updates,
    rules::Rules,
    show_notification,
    upload::{find_local_deletions, upload},
    Ctx,
};
use anyhow::{Context, Result};
use chrono::{TimeDelta, Utc};
use humantime::format_duration;
use itertools::Itertools;
use tracing::warn;

pub async fn sync(ctx: &Arc<Ctx>, dry_run: bool) -> Result<()> {
    sync_inner(ctx, dry_run).await.inspect_err(|err| {
        if ctx.config.enable_desktop_notifications {
            if dry_run {
                show_notification("rammingen dry run failed", &err.to_string());
            } else {
                show_notification("rammingen dry run failed", &err.to_string());
            }
        }
    })
}

async fn sync_inner(ctx: &Arc<Ctx>, dry_run: bool) -> Result<()> {
    let mut existing_paths = HashSet::new();
    let mut mount_points = ctx
        .config
        .mount_points
        .iter()
        .map(|mount_point| {
            let rules = Rules::new(
                &[&ctx.config.always_exclude, &mount_point.exclude],
                mount_point.local_path.clone(),
            );
            (mount_point, rules)
        })
        .collect_vec();

    for (mount_point, rules) in &mut mount_points {
        upload(
            ctx,
            &mount_point.local_path,
            &mount_point.archive_path,
            rules,
            true,
            &mut existing_paths,
            dry_run,
        )
        .await?;
    }
    find_local_deletions(ctx, &mut mount_points, &existing_paths, dry_run).await?;
    pull_updates(ctx).await?;
    for mount_point in &ctx.config.mount_points {
        download_latest(
            ctx,
            &mount_point.archive_path,
            &mount_point.local_path,
            &mut Rules::new(
                &[&ctx.config.always_exclude, &mount_point.exclude],
                mount_point.local_path.clone(),
            ),
            true,
            dry_run,
        )
        .await?;
    }
    if ctx.config.enable_desktop_notifications {
        if dry_run {
            let report =
                NotificationCounters::from(&ctx.final_counters).report(dry_run, false, &ctx);
            show_notification("rammingen dry run complete", &report);
        } else {
            let mut stats = ctx
                .db
                .notification_stats()
                .inspect_err(|err| {
                    warn!("failed to load notification stats from db: {err}");
                })
                .unwrap_or_default();
            stats.pending_counters += NotificationCounters::from(&ctx.final_counters);
            stats.pending_counters.completed_syncs += 1;
            let now = Utc::now();
            let desktop_notification_interval =
                TimeDelta::from_std(ctx.config.desktop_notification_interval)
                    .context("config.desktop_notification_interval out of range")?;
            let show = stats.last_notified_at.map_or(true, |last_notified_at| {
                (now - last_notified_at) > desktop_notification_interval
            });
            if show {
                let has_interval = ctx.config.desktop_notification_interval != Duration::ZERO;
                let interval_str = if has_interval {
                    format!(
                        "Stats for the last {}:\n",
                        format_duration(ctx.config.desktop_notification_interval),
                    )
                } else {
                    "".to_string()
                };
                show_notification(
                    "rammingen sync complete",
                    &format!(
                        "{}{}",
                        interval_str,
                        stats.pending_counters.report(dry_run, has_interval, ctx)
                    ),
                );
                stats.last_notified_at = Some(now);
                stats.pending_counters = NotificationCounters::default();
            }
            ctx.db.set_notification_stats(&stats)?;
        }
    }
    Ok(())
}
