use std::collections::HashSet;

use crate::{
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
    // set_status("Checking for files deleted remotely");
    // for entry in ctx.db.get_all_archive_entries().rev() {
    //     let entry = entry?;
    //     if entry.kind.is_some() {
    //         continue;
    //     }

    // }
    Ok(())
}
