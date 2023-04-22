use std::collections::HashSet;

use crate::{
    download::download,
    pull_updates::pull_updates,
    upload::{find_local_deletions, upload},
    Ctx,
};
use anyhow::Result;

pub async fn sync(ctx: &Ctx) -> Result<()> {
    let mut existing_paths = HashSet::new();
    for mount_point in &ctx.config.mount_points {
        upload(
            ctx,
            &mount_point.local,
            &mount_point.archive,
            &mount_point.rules,
            true,
            &mut existing_paths,
        )
        .await?;
    }
    find_local_deletions(ctx, &existing_paths).await?;
    pull_updates(ctx).await?;
    for mount_point in &ctx.config.mount_points {
        download(
            ctx,
            &mount_point.archive,
            &mount_point.local,
            &mount_point.rules,
            true,
        )
        .await?;
    }
    Ok(())
}
