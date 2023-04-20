use std::collections::{HashMap, HashSet};

use crate::{
    config::MountPoint,
    rules::Rules,
    term::set_status,
    upload::{upload, SanitizedLocalPath},
    Ctx,
};
use anyhow::Result;
use rammingen_protocol::ArchivePath;

fn to_archive_path<'a>(
    local_path: &SanitizedLocalPath,
    mount_points: &'a [MountPoint],
    cache: &mut HashMap<SanitizedLocalPath, Option<(ArchivePath, &'a Rules)>>,
) -> Option<(ArchivePath, &'a Rules)> {
    if let Some(value) = cache.get(local_path) {
        return value.clone();
    }
    let output = if let Some(mount_point) = mount_points.iter().find(|mp| &mp.local == local_path) {
        if mount_point.rules.eval(local_path) {
            Some((mount_point.archive.clone(), &mount_point.rules))
        } else {
            None
        }
    } else if let Some(parent) = local_path.parent() {
        if let Some((archive_parent, rules)) = to_archive_path(&parent, mount_points, cache) {
            if rules.eval(local_path) {
                let new_archive_path = archive_parent
                    .join(local_path.file_name())
                    .expect("failed to join archive path");
                Some((new_archive_path, rules))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    cache.insert(local_path.clone(), output.clone());
    output
}

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
    set_status("Checking for files deleted locally");
    for entry in ctx.db.get_local_entries() {
        let (local_path, _data) = entry?;
        if existing_paths.contains(&local_path) {
            continue;
        }
        let Some((_archive_path, _)) =
            to_archive_path(&local_path, &ctx.config.mount_points, &mut HashMap::new())
            else {
                continue;
            };
        // AddVersion {
        //     path: archive_path,
        //     record_trigger: RecordTrigger::Sync,
        //     kind: todo!(),
        //     exists: todo!(),
        //     content: todo!(),
        // };
    }
    Ok(())
}
