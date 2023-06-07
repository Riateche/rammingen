use std::collections::HashSet;

use crate::{
    download::download_latest,
    pull_updates::pull_updates,
    rules::Rules,
    upload::{find_local_deletions, upload},
    Ctx,
};
use anyhow::Result;
use itertools::Itertools;

pub async fn sync(ctx: &Ctx, dry_run: bool) -> Result<()> {
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
    Ok(())
}
