use std::collections::HashSet;

use crate::{
    download::download,
    pull_updates::pull_updates,
    rules::Rules,
    upload::{find_local_deletions, upload},
    Ctx,
};
use anyhow::Result;
use itertools::Itertools;

pub async fn sync(ctx: &Ctx) -> Result<()> {
    let mut existing_paths = HashSet::new();
    let mut mount_points = ctx
        .config
        .mount_points
        .iter()
        .map(|mount_point| {
            let rules = Rules::new(
                &[&ctx.config.global_rules, &mount_point.rules],
                mount_point.local.clone(),
            );
            (mount_point, rules)
        })
        .collect_vec();

    for (mount_point, rules) in &mut mount_points {
        upload(
            ctx,
            &mount_point.local,
            &mount_point.archive,
            rules,
            true,
            &mut existing_paths,
        )
        .await?;
    }
    find_local_deletions(ctx, &mut mount_points, &existing_paths).await?;
    pull_updates(ctx).await?;
    for mount_point in &ctx.config.mount_points {
        download(
            ctx,
            &mount_point.archive,
            &mount_point.local,
            &mut Rules::new(
                &[&ctx.config.global_rules, &mount_point.rules],
                mount_point.local.clone(),
            ),
            true,
        )
        .await?;
    }
    Ok(())
}
