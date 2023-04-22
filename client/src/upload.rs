use anyhow::{anyhow, bail, Result};
use fs_err as fs;
use futures::future::BoxFuture;
use rammingen_protocol::{
    AddVersion, ArchivePath, ContentHashExists, DateTime, EntryKind, FileContent, RecordTrigger,
};
use std::{collections::HashSet, path::Component, sync::atomic::Ordering, time::Duration};
use tokio::{task::block_in_place, time::sleep};

use crate::{
    config::MountPoint,
    db::LocalEntryInfo,
    encryption::{self, encrypt_path},
    path::SanitizedLocalPath,
    rules::Rules,
    term::{debug, info, set_status, warn},
    Ctx,
};

const TOO_RECENT_INTERVAL: Duration = Duration::from_secs(3);

fn to_archive_path<'a>(
    local_path: &SanitizedLocalPath,
    mount_points: &'a mut [(&MountPoint, Rules)],
) -> Result<Option<(ArchivePath, &'a mut Rules)>> {
    for (mount_point, rules) in mount_points {
        if let Ok(relative) = local_path.as_path().strip_prefix(&mount_point.local_path) {
            let mut archive = mount_point.archive_path.clone();
            for component in relative.components() {
                if let Component::Normal(name) = component {
                    archive = archive.join(name.to_str().expect("sanitized"))?;
                } else {
                    bail!("unexpected non-normal component in {:?}", relative);
                };
            }
            return Ok(Some((archive, rules)));
        }
    }
    Ok(None)

    // if let Some(value) = cache.get(local_path) {
    //     return value.clone();
    // }
    // let output = if let Some(mount_point) = mount_points.iter().find(|mp| &mp.local == local_path) {
    //     if mount_point.rules.eval(local_path) {
    //         Some((mount_point.archive.clone(), &mount_point.rules))
    //     } else {
    //         None
    //     }
    // } else if let Some(parent) = local_path.parent()? {
    //     if let Some((archive_parent, rules)) = to_archive_path(&parent, mount_points, cache) {
    //         if rules.eval(local_path) {
    //             let new_archive_path = archive_parent
    //                 .join(local_path.file_name())
    //                 .expect("failed to join archive path");
    //             Some((new_archive_path, rules))
    //         } else {
    //             None
    //         }
    //     } else {
    //         None
    //     }
    // } else {
    //     None
    // };

    // cache.insert(local_path.clone(), output.clone());
    // output
}

pub async fn find_local_deletions<'a>(
    ctx: &'a Ctx,
    mount_points: &'a mut [(&MountPoint, Rules)],
    existing_paths: &'a HashSet<SanitizedLocalPath>,
) -> Result<()> {
    set_status("Checking for files deleted locally");
    for entry in ctx.db.get_all_local_entries().rev() {
        let (local_path, _data) = entry?;
        if existing_paths.contains(&local_path) {
            continue;
        }

        let Some((archive_path, rules)) =
            to_archive_path(&local_path, mount_points)?
            else {
                continue;
            };
        if rules.matches(&local_path)? {
            continue;
        }
        let id = ctx
            .client
            .request(&AddVersion {
                path: archive_path,
                record_trigger: RecordTrigger::Sync,
                kind: None,
                content: None,
            })
            .await?;
        if id.is_some() {
            ctx.counters
                .updated_on_server
                .fetch_add(1, Ordering::Relaxed);
            info(format!("Recorded deletion of {}", local_path));
        }
        ctx.db.remove_local_entry(&local_path)?;
    }
    Ok(())
}

pub fn upload<'a>(
    ctx: &'a Ctx,
    local_path: &'a SanitizedLocalPath,
    archive_path: &'a ArchivePath,
    rules: &'a mut Rules,
    is_mount: bool,
    existing_paths: &'a mut HashSet<SanitizedLocalPath>,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        set_status(format!("Scanning local files: {}", local_path));
        existing_paths.insert(local_path.clone());
        let metadata = fs::symlink_metadata(local_path)?;
        if metadata.is_symlink() {
            warn(format!("skipping symlink: {}", local_path));
            return Ok(());
        }
        if rules.matches(local_path)? {
            debug(format!("ignored: {}", local_path));
            return Ok(());
        }
        ctx.counters.scanned_entries.fetch_add(1, Ordering::Relaxed);
        let is_dir = metadata.is_dir();
        let kind = if is_dir {
            EntryKind::Directory
        } else {
            EntryKind::File
        };
        let db_data = ctx.db.get_local_entry(local_path)?;

        let changed;
        let content;

        if is_dir {
            changed = db_data
                .as_ref()
                .map_or(true, |db_data| db_data.kind != kind);
            content = None;
        } else {
            let mut modified = None;
            for _ in 0..5 {
                let new_modified = fs::symlink_metadata(local_path)?.modified()?;
                if new_modified.elapsed()? < TOO_RECENT_INTERVAL {
                    info(format!(
                        "file {} was modified recently, waiting...",
                        local_path
                    ));
                    sleep(TOO_RECENT_INTERVAL).await;
                } else {
                    modified = Some(new_modified);
                    break;
                }
            }
            let modified =
                modified.ok_or_else(|| anyhow!("file {:?} keeps updating", local_path))?;
            let modified_datetime = DateTime::from(modified);

            let maybe_changed = db_data.as_ref().map_or(true, |db_data| {
                db_data.kind != kind || {
                    db_data
                        .content
                        .as_ref()
                        .map_or(true, |content| content.modified_at != modified_datetime)
                }
            });

            if maybe_changed {
                let file_data = block_in_place(|| {
                    encryption::encrypt_file(local_path, &ctx.cipher, &ctx.config.salt)
                })?;

                let final_modified = fs::symlink_metadata(local_path)?.modified()?;
                if final_modified != modified {
                    bail!(
                        "file {:?} was updated while it was being processed",
                        local_path
                    );
                }

                let current_content = FileContent {
                    modified_at: modified_datetime,
                    size: file_data.size,
                    hash: file_data.hash,
                    unix_mode: unix_mode(&metadata),
                };

                changed = db_data.as_ref().map_or(true, |db_data| {
                    db_data.kind != kind || {
                        db_data.content.as_ref().map_or(true, |content| {
                            content.hash != current_content.hash
                                || content.unix_mode != current_content.unix_mode
                        })
                    }
                });

                if changed
                    && !ctx
                        .client
                        .request(&ContentHashExists(current_content.hash.clone()))
                        .await?
                {
                    ctx.client
                        .upload(&current_content.hash, file_data.file)
                        .await?;
                }

                content = Some(current_content);
            } else {
                changed = false;
                content = None;
            }
        };

        if changed {
            let add_version = AddVersion {
                path: encrypt_path(archive_path, &ctx.cipher)?,
                record_trigger: RecordTrigger::Upload,
                kind: Some(kind),
                content: content.clone(),
            };
            ctx.counters.sent_to_server.fetch_add(1, Ordering::Relaxed);
            if ctx.client.request(&add_version).await?.is_some() {
                ctx.counters
                    .updated_on_server
                    .fetch_add(1, Ordering::Relaxed);
                info(format!("Uploaded {}", local_path));
            }
            if is_mount {
                ctx.db
                    .set_local_entry(local_path, &LocalEntryInfo { kind, content })?;
            }
        }
        if is_dir {
            for entry in fs::read_dir(local_path)? {
                let entry = entry?;
                let file_name = entry.file_name();
                let file_name_str = file_name
                    .to_str()
                    .ok_or_else(|| anyhow!("Unsupported file name: {:?}", entry.path()))?;
                let entry_local_path = local_path.join(file_name_str)?;
                let entry_archive_path = archive_path.join(file_name_str).map_err(|err| {
                    anyhow!(
                        "Failed to construct archive path for {:?}: {:?}",
                        entry.path(),
                        err
                    )
                })?;
                upload(
                    ctx,
                    &entry_local_path,
                    &entry_archive_path,
                    rules,
                    is_mount,
                    existing_paths,
                )
                .await
                .map_err(|err| anyhow!("Failed to process {:?}: {:?}", entry.path(), err))?;
            }
        }
        Ok(())
    })
}

#[cfg(target_family = "unix")]
fn unix_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::prelude::PermissionsExt;

    Some(metadata.permissions().mode())
}

#[cfg(not(target_family = "unix"))]
fn unix_mode(_metadata: &Metadata) -> Option<u32> {
    None
}
