use anyhow::Result;
use itertools::Itertools;
use rammingen_protocol::ArchivePath;

use crate::{
    encryption::encrypt_path, path::SanitizedLocalPath, pull_updates::pull_updates, rules::Rules,
    term::info, upload::to_archive_path, Ctx,
};

pub async fn local_status(ctx: &Ctx, path: &SanitizedLocalPath) -> Result<()> {
    pull_updates(ctx).await?;
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

    info(format!("normalized local path: {}", path));

    if let Some((archive_path, rules)) = to_archive_path(path, &mut mount_points)? {
        if rules.matches(path)? {
            info("this path is ignored according to the configured exclude rules");
        } else {
            info(format!("archive path: {}", archive_path));
            let encrypted = encrypt_path(&archive_path, &ctx.cipher)?;
            info(format!("encrypted archive path: {}", encrypted));
            info(format!(
                "archive entry in local db: {:?}",
                ctx.db.get_archive_entry(&archive_path)?
            ));
            info(format!(
                "local entry in local db: {:?}",
                ctx.db.get_local_entry(path)?
            ));
        }
    } else {
        info("this path is not inside any of the configured mount points");
    }

    Ok(())
}

pub async fn ls(_ctx: &Ctx, _path: &ArchivePath) -> Result<()> {
    Ok(())
}
