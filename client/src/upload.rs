use anyhow::{anyhow, bail, Result};
use fs_err as fs;
use futures::future::BoxFuture;
use rammingen_protocol::{
    AddVersion, ArchivePath, ContentHashExists, EntryKind, FileContent, RecordTrigger,
};
use std::{
    fmt::Display,
    path::{Path, PathBuf, MAIN_SEPARATOR},
    sync::atomic::Ordering,
    time::Duration,
};
use tokio::{task::block_in_place, time::sleep};

use crate::{
    encryption::{self, encrypt_path},
    rules::Rules,
    term::{debug, info, set_status, warn},
    Ctx,
};

const TOO_RECENT_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct SanitizedLocalPath(pub String);

impl From<SanitizedLocalPath> for PathBuf {
    fn from(value: SanitizedLocalPath) -> Self {
        value.0.into()
    }
}

impl From<&SanitizedLocalPath> for PathBuf {
    fn from(value: &SanitizedLocalPath) -> Self {
        value.0.clone().into()
    }
}

impl AsRef<Path> for SanitizedLocalPath {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl Display for SanitizedLocalPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl SanitizedLocalPath {
    pub fn new(path: &Path) -> Result<Self> {
        let path = if path.try_exists()? {
            dunce::canonicalize(path)
                .map_err(|e| anyhow!("failed to canonicalize {:?}: {}", path, e))?
        } else {
            let parent = path
                .parent()
                .ok_or_else(|| anyhow!("unsupported path (couldn't get parent): {:?}", path))?;
            let file_name = path
                .file_name()
                .ok_or_else(|| anyhow!("unsupported path (couldn't get parent): {:?}", path))?;
            let parent = dunce::canonicalize(parent)
                .map_err(|e| anyhow!("failed to canonicalize {:?}: {}", parent, e))?;
            parent.join(file_name)
        };

        let str = path
            .to_str()
            .ok_or_else(|| anyhow!("unsupported path: {:?}", path))?;
        if str.is_empty() {
            bail!("path cannot be empty");
        }
        Ok(Self(str.into()))
    }

    pub fn join(&self, file_name: &str) -> Result<Self> {
        if file_name.is_empty() {
            bail!("file name cannot be empty");
        }
        if file_name.contains('/') {
            bail!("file name cannot contain '/'");
        }
        if file_name.contains('\\') {
            bail!("file name cannot contain '\\'");
        }
        let mut path = self.clone();
        path.0.push(MAIN_SEPARATOR);
        path.0.push_str(file_name);
        Ok(path)
    }

    pub fn file_name(&self) -> &str {
        self.0
            .split(MAIN_SEPARATOR)
            .rev()
            .next()
            .expect("cannot be empty")
    }
}

pub fn upload<'a>(
    ctx: &'a Ctx,
    local_path: &'a SanitizedLocalPath,
    archive_path: &'a ArchivePath,
    rules: &'a Rules,
    is_mount: bool,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        set_status(format!("Uploading {}", local_path));
        let metadata = fs::symlink_metadata(local_path)?;
        if metadata.is_symlink() {
            warn(format!("skipping symlink: {}", local_path));
            return Ok(());
        }
        if !rules.eval(local_path) {
            debug(format!("ignored: {}", local_path));
            return Ok(());
        }
        ctx.counters.scanned_entries.fetch_add(1, Ordering::Relaxed);
        let is_dir = metadata.is_dir();
        let content = if is_dir {
            None
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
        ctx.counters.sent_to_server.fetch_add(1, Ordering::Relaxed);
        if ctx.client.request(&add_version).await?.is_some() {
            ctx.counters
                .updated_on_server
                .fetch_add(1, Ordering::Relaxed);
            info(format!("Uploaded new version of {}", local_path));
        }
        if is_mount {}
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
                upload(ctx, &entry_local_path, &entry_archive_path, rules, is_mount)
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
