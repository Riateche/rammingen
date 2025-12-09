use {
    crate::{
        Ctx,
        counters::NotificationCounters,
        download::download_latest,
        pull_updates::pull_updates,
        rules::Rules,
        show_notification,
        upload::{find_local_deletions, upload},
    },
    anyhow::{Context, Result},
    cadd::ops::Cadd,
    chrono::{TimeDelta, Utc},
    humantime::format_duration,
    itertools::Itertools,
    std::{collections::HashSet, sync::Arc, time::Duration},
    tracing::warn,
};

pub async fn sync(ctx: &Arc<Ctx>, dry_run: bool) -> Result<()> {
    sync_inner(ctx, dry_run).await.inspect_err(|err| {
        if ctx.config.enable_desktop_notifications {
            if dry_run {
                show_notification("rammingen dry run failed", &err.to_string());
            } else {
                let since_last_sync_text = since_last_sync_text(ctx)
                    .inspect_err(|error| warn!(?error, "since_last_sync_text failed"))
                    .unwrap_or_default();

                let mut text = format!("{err:?}");
                if !since_last_sync_text.is_empty() {
                    text.push('\n');
                    text.push_str(&since_last_sync_text);
                }

                show_notification("rammingen sync failed", &text);
            }
        }
    })
}

pub fn since_last_sync_text(ctx: &Ctx) -> anyhow::Result<String> {
    let Some(last_sync_at) = ctx.db.notification_stats()?.last_successful_sync_at else {
        return Ok(String::new());
    };
    let since_last_sync = Utc::now().signed_duration_since(last_sync_at).to_std()?;
    Ok(format!(
        "Last successful sync was {} ago",
        format_duration(since_last_sync)
    ))
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
        if mount_point.local_path.try_exists_nofollow()? {
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
            let report = NotificationCounters::from(&ctx.final_counters).report(dry_run, ctx);
            show_notification("rammingen dry run complete", &report);
        } else {
            let mut stats = ctx
                .db
                .notification_stats()
                .inspect_err(|err| {
                    warn!("Failed to load notification stats from db: {err}");
                })
                .unwrap_or_default();
            stats
                .pending_counters
                .cadd(&NotificationCounters::from(&ctx.final_counters))?;
            stats.pending_counters.completed_syncs = stats
                .pending_counters
                .completed_syncs
                .cadd(1u64)
                .context("completed_syncs overflow")?;
            let now = Utc::now();
            let desktop_notification_interval =
                TimeDelta::from_std(ctx.config.desktop_notification_interval)
                    .context("config.desktop_notification_interval out of range")?;
            let show = stats.last_notified_at.is_none_or(|last_notified_at| {
                now.signed_duration_since(last_notified_at) > desktop_notification_interval
            });
            if show {
                let has_interval = ctx.config.desktop_notification_interval != Duration::ZERO;
                let interval_str = if has_interval {
                    format!(
                        "Stats for the last {}:\n",
                        format_duration(ctx.config.desktop_notification_interval),
                    )
                } else {
                    String::new()
                };
                show_notification(
                    "rammingen sync complete",
                    &format!(
                        "{}{}",
                        interval_str,
                        stats.pending_counters.report(dry_run, ctx)
                    ),
                );
                stats.last_notified_at = Some(now);
                stats.pending_counters = NotificationCounters::default();
            }
            stats.last_successful_sync_at = Some(Utc::now());
            ctx.db.set_notification_stats(&stats)?;
        }
    }
    Ok(())
}
