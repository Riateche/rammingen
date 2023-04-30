use anyhow::Result;
use itertools::Itertools;

use crate::{
    encryption::encrypt_path, path::SanitizedLocalPath, rules::Rules, term::info,
    upload::to_archive_path, Ctx,
};

pub async fn ls(ctx: &Ctx, path: &str) -> Result<()> {
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

    if path.starts_with("ar:") {
        // TODO
    } else {
        let path = SanitizedLocalPath::new(path)?;
        info(format!("normalized local path: {}", path));

        if let Some((archive_path, rules)) = to_archive_path(&path, &mut mount_points)? {
            if rules.matches(&path)? {
                info("this path is ignored according to the configured exclude rules");
            } else {
                info(format!("archive path: {}", archive_path.0));
                let encrypted = encrypt_path(&archive_path, &ctx.cipher)?;
                info(format!("encrypted archive path: {}", encrypted.0));
            }
        } else {
            info("this path is not inside any of the configured mount points");
        }
    }

    Ok(())
}
