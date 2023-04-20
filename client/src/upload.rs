use anyhow::{anyhow, bail, Result};
use fs_err as fs;
use futures::future::BoxFuture;
use rammingen_protocol::{
    AddVersion, ArchivePath, ContentHashExists, DateTime, EntryKind, FileContent, RecordTrigger,
};
use serde::{de::Error, Deserialize};
use std::{
    collections::HashSet,
    fmt::Display,
    path::{Path, PathBuf, MAIN_SEPARATOR},
    sync::atomic::Ordering,
    time::Duration,
};
use tokio::{task::block_in_place, time::sleep};

use crate::{
    db::LocalEntryInfo,
    encryption::{self, encrypt_path},
    rules::Rules,
    term::{debug, info, set_status, warn},
    Ctx,
};

const TOO_RECENT_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
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

    pub fn parent(&self) -> Option<Self> {
        Path::new(&self.0).parent().map(|parent| {
            Self(
                parent
                    .to_str()
                    .expect("parent of sanitized path must be valid utf-8")
                    .into(),
            )
        })
    }
}

impl<'de> Deserialize<'de> for SanitizedLocalPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        Self::new(string).map_err(D::Error::custom)
    }
}

pub fn upload<'a>(
    ctx: &'a Ctx,
    local_path: &'a SanitizedLocalPath,
    archive_path: &'a ArchivePath,
    rules: &'a Rules,
    is_mount: bool,
    existing_paths: &'a mut HashSet<SanitizedLocalPath>,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        set_status(format!("Uploading {}", local_path));
        existing_paths.insert(local_path.clone());
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
                content,
            };
            ctx.counters.sent_to_server.fetch_add(1, Ordering::Relaxed);
            if ctx.client.request(&add_version).await?.is_some() {
                ctx.counters
                    .updated_on_server
                    .fetch_add(1, Ordering::Relaxed);
                info(format!("Uploaded new version of {}", local_path));
            }
            if is_mount {
                ctx.db.set_local_entry(
                    local_path,
                    &LocalEntryInfo {
                        kind,
                        content: add_version.content.clone(),
                    },
                )?;
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
