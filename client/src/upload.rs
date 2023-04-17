use anyhow::{anyhow, bail, Result};
use fs_err as fs;
use futures::future::BoxFuture;
use rammingen_protocol::{
    AddVersion, ArchivePath, ContentHashExists, EntryKind, FileContent, RecordTrigger,
};
use std::{path::Path, time::Duration};
use tokio::{task::block_in_place, time::sleep};
use tracing::{info, warn};

use crate::{
    encryption::{self, encrypt_path},
    Ctx,
};

const TOO_RECENT_INTERVAL: Duration = Duration::from_secs(3);

pub fn upload<'a>(
    ctx: &'a Ctx,
    local_path: &'a Path,
    archive_path: &'a ArchivePath,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let metadata = fs::symlink_metadata(local_path)?;
        if metadata.is_symlink() {
            info!("skipping symlink: {:?}", local_path);
            return Ok(());
        }
        let is_dir = metadata.is_dir();
        let content = if is_dir {
            None
        } else {
            let mut modified = None;
            for _ in 0..5 {
                let new_modified = fs::symlink_metadata(local_path)?.modified()?;
                if new_modified.elapsed()? < TOO_RECENT_INTERVAL {
                    info!("file {:?} was modified recently, waiting...", local_path);
                    sleep(TOO_RECENT_INTERVAL).await;
                } else {
                    modified = Some(new_modified);
                    break;
                }
            }
            let modified =
                modified.ok_or_else(|| anyhow!("file {:?} keeps updating", local_path))?;

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

            if !ctx
                .client
                .request(&ContentHashExists(file_data.hash.clone()))
                .await?
            {
                ctx.client.upload(&file_data.hash, file_data.file).await?;
            }

            Some(FileContent {
                modified_at: modified.into(),
                size: file_data.size,
                hash: file_data.hash,
                unix_mode: unix_mode(&metadata),
            })
        };

        let add_version = AddVersion {
            path: encrypt_path(archive_path, &ctx.cipher)?,
            record_trigger: RecordTrigger::Upload,
            kind: if is_dir {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            exists: true,
            content,
        };
        if ctx.client.request(&add_version).await?.is_some() {
            info!("uploaded new version of {:?}", local_path);
        }
        if is_dir {
            for entry in fs::read_dir(local_path)? {
                let entry = entry?;
                let file_name = entry.file_name();
                let Some(file_name_str) = file_name.to_str() else {
                warn!("unsupported file name at {:?}", entry.path());
                continue
            };
                let entry_archive_path = match archive_path.join(file_name_str) {
                    Ok(path) => path,
                    Err(err) => {
                        warn!(
                            ?err,
                            "failed to construct archive path for {:?}",
                            entry.path()
                        );
                        continue;
                    }
                };
                if let Err(err) = upload(ctx, &entry.path(), &entry_archive_path).await {
                    warn!(?err, "failed to process {:?}", entry.path());
                }
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
