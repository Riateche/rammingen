use {
    crate::{
        config::MountPoint,
        info::pretty_size,
        path::SanitizedLocalPath,
        rules::Rules,
        term::{set_status, set_status_updater},
        unix_mode, Ctx,
    },
    anyhow::{anyhow, bail, Result},
    fs::symlink_metadata,
    fs_err as fs,
    futures::future::BoxFuture,
    rammingen_protocol::{
        endpoints::{AddVersion, AddVersions, ContentHashExists},
        util::{interrupt_on_error, native_to_archive_relative_path, ErrorSender},
        ArchivePath, ContentHash, DateTimeUtc, EntryKind, FileContent, RecordTrigger,
    },
    rammingen_sdk::content::{DecryptedContentHead, EncryptedFileHead, LocalEntry},
    std::{
        collections::HashSet,
        fs::FileType,
        mem,
        sync::{atomic::Ordering, Arc},
        time::Duration,
    },
    tokio::{
        sync::{mpsc, oneshot, Semaphore},
        task::{self, block_in_place},
        time::sleep,
    },
    tracing::{debug, info, warn},
};

const TOO_RECENT_INTERVAL: Duration = Duration::from_millis(100);
const BATCH_SIZE: usize = 128;

pub fn to_archive_path<'a>(
    local_path: &SanitizedLocalPath,
    mount_points: &'a mut [(&MountPoint, Rules)],
) -> Result<Option<(ArchivePath, &'a mut Rules)>> {
    for (mount_point, rules) in mount_points {
        if local_path == &mount_point.local_path {
            return Ok(Some((mount_point.archive_path.clone(), rules)));
        }
        if let Ok(relative) = local_path.as_path().strip_prefix(&mount_point.local_path) {
            let archive = mount_point
                .archive_path
                .join_multiple(&native_to_archive_relative_path(relative)?)?;
            return Ok(Some((archive, rules)));
        }
    }
    Ok(None)
}

pub async fn find_local_deletions<'a>(
    ctx: &'a Ctx,
    mount_points: &'a mut [(&MountPoint, Rules)],
    existing_paths: &'a HashSet<SanitizedLocalPath>,
    dry_run: bool,
) -> Result<()> {
    let _status = set_status("Checking for files deleted locally");
    let mut new_versions = Vec::new();
    let mut local_paths = Vec::new();

    for entry in ctx.db.get_all_local_entries().rev() {
        let (local_path, _data) = match entry {
            Ok(r) => r,
            Err(err) => {
                warn!("Couldn't load a local entry: {err}");
                continue;
            }
        };
        if existing_paths.contains(&local_path) {
            continue;
        }

        let Some((archive_path, rules)) = to_archive_path(&local_path, mount_points)? else {
            continue;
        };
        if rules.matches(&local_path)? {
            continue;
        }
        if dry_run {
            info!("Would record deletion of {}", local_path);
            ctx.final_counters
                .uploaded_entries
                .fetch_add(1, Ordering::Relaxed);
        } else {
            new_versions.push(AddVersion {
                path: ctx.cipher.encrypt_path(&archive_path)?,
                record_trigger: RecordTrigger::Sync,
                kind: None,
                content: None,
            });
            local_paths.push(local_path);
            if new_versions.len() >= BATCH_SIZE {
                record_deletion_batch(ctx, &mut new_versions, &mut local_paths).await?;
            }
        }
    }
    record_deletion_batch(ctx, &mut new_versions, &mut local_paths).await?;
    Ok(())
}

async fn record_deletion_batch(
    ctx: &Ctx,
    new_versions: &mut Vec<AddVersion>,
    local_paths: &mut Vec<SanitizedLocalPath>,
) -> Result<()> {
    if new_versions.is_empty() {
        return Ok(());
    }
    let results = ctx
        .client
        .request(&AddVersions(mem::take(new_versions)))
        .await?;
    if results.len() != local_paths.len() {
        bail!("invalid item count in AddVersions response");
    }
    for (local_path, response) in local_paths.drain(..).zip(results) {
        if response.added {
            ctx.final_counters
                .uploaded_entries
                .fetch_add(1, Ordering::Relaxed);
            info!("Recorded deletion of {}", local_path);
        }
        ctx.db.remove_local_entry(&local_path)?;
    }
    Ok(())
}

pub async fn upload(
    ctx: &Arc<Ctx>,
    local_path: &SanitizedLocalPath,
    archive_path: &ArchivePath,
    rules: &mut Rules,
    is_mount: bool,
    existing_paths: &mut HashSet<SanitizedLocalPath>,
    dry_run: bool,
) -> Result<()> {
    interrupt_on_error(|error_sender| async move {
        let ctx2 = ctx.clone();
        let (content_sender, content_receiver) = mpsc::channel(100_000);

        let content_upload_task = task::spawn(content_upload_task(
            ctx.clone(),
            content_receiver,
            error_sender.clone(),
        ));

        let (versions_sender, versions_receiver) = mpsc::channel(100_000);
        let versions_task = task::spawn(add_versions_task(
            ctx.clone(),
            versions_receiver,
            error_sender,
        ));

        let mut ctx = UploadContext {
            ctx,
            rules,
            is_mount,
            existing_paths,
            dry_run,
            content_upload_sender: content_sender,
            add_versions_sender: versions_sender,
        };
        upload_inner(&mut ctx, local_path, archive_path).await?;
        drop(ctx);
        let _status = set_status_updater(move || {
            let queued = ctx2
                .intermediate_counters
                .queued_upload_entries
                .load(Ordering::Relaxed);
            let unqueued = ctx2
                .intermediate_counters
                .unqueued_upload_entries
                .load(Ordering::Relaxed);
            format!("Uploading ({unqueued} / {queued} entries)")
        });
        content_upload_task.await?;
        versions_task.await?;
        Ok(())
    })
    .await
}

struct UploadContext<'a> {
    ctx: &'a Ctx,
    rules: &'a mut Rules,
    is_mount: bool,
    existing_paths: &'a mut HashSet<SanitizedLocalPath>,
    dry_run: bool,
    content_upload_sender: mpsc::Sender<ContentUploadTaskItem>,
    add_versions_sender: mpsc::Sender<(AddVersionsTaskItem, Option<oneshot::Receiver<()>>)>,
}

fn upload_inner<'a>(
    ctx: &'a mut UploadContext<'_>,
    local_path: &'a SanitizedLocalPath,
    archive_path: &'a ArchivePath,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let _status = set_status(format!("Scanning local files: {local_path}"));
        ctx.existing_paths.insert(local_path.clone());
        let mut metadata = fs::symlink_metadata(local_path)?;
        if is_special_file(&metadata.file_type()) {
            debug!("Skipping special file: {}", local_path);
            return Ok(());
        }
        if metadata.is_symlink() {
            debug!("Skipping symlink: {}", local_path);
            return Ok(());
        }
        if ctx.rules.matches(local_path)? {
            debug!("Ignored: {}", local_path);
            return Ok(());
        }
        let is_dir = metadata.is_dir();
        let kind = if is_dir {
            EntryKind::Directory
        } else {
            EntryKind::File
        };
        let db_data = ctx.ctx.db.get_local_entry(local_path)?;

        let changed;
        let content;
        let oneshot_receiver;

        if is_dir {
            changed = db_data.as_ref().is_none_or(|db_data| db_data.kind != kind);
            content = None;
            oneshot_receiver = None;
        } else {
            let mut modified = None;
            for _ in 0..5 {
                metadata = fs::symlink_metadata(local_path)?;
                let new_modified = metadata.modified()?;
                if new_modified.elapsed()? < TOO_RECENT_INTERVAL {
                    info!("File {} was modified recently, waiting...", local_path);
                    sleep(TOO_RECENT_INTERVAL).await;
                } else {
                    modified = Some(new_modified);
                    break;
                }
            }
            let modified =
                modified.ok_or_else(|| anyhow!("file {:?} keeps updating", local_path))?;
            let modified_datetime = DateTimeUtc::from(modified);
            let unix_mode = unix_mode(&metadata);

            let maybe_changed = db_data.as_ref().is_none_or(|db_data| {
                db_data.kind != kind || {
                    db_data.content.as_ref().is_none_or(|content| {
                        content.modified_at != modified_datetime || content.unix_mode != unix_mode
                    })
                }
            });

            if maybe_changed {
                let file_data = block_in_place(|| ctx.ctx.cipher.encrypt_file(local_path))?;

                let final_modified = fs::symlink_metadata(local_path)?.modified()?;
                if final_modified != modified {
                    bail!(
                        "file {:?} was updated while it was being processed",
                        local_path
                    );
                }

                let current_content = DecryptedContentHead {
                    modified_at: modified_datetime,
                    original_size: file_data.original_size,
                    encrypted_size: file_data.encrypted_size,
                    hash: file_data.hash.clone(),
                    unix_mode,
                };

                changed = db_data.as_ref().is_none_or(|db_data| {
                    db_data.kind != kind || {
                        db_data.content.as_ref().is_none_or(|content| {
                            content.hash != current_content.hash
                                || content.unix_mode != current_content.unix_mode
                        })
                    }
                });

                if changed {
                    if ctx.dry_run {
                        if file_data.encrypted_size
                            > ctx.ctx.config.warn_about_files_larger_than.get_bytes()
                        {
                            warn!(
                                "Would upload {} file: {}",
                                pretty_size(file_data.encrypted_size),
                                local_path
                            );
                        }
                        oneshot_receiver = None;
                    } else {
                        let (sender, receiver) = oneshot::channel();
                        ctx.content_upload_sender
                            .send(ContentUploadTaskItem {
                                hash: current_content.hash.clone(),
                                local_path: local_path.clone(),
                                file_data,
                                sender,
                            })
                            .await
                            .map_err(|_| anyhow!("failed to send item to content upload task"))?;
                        oneshot_receiver = Some(receiver);
                    }
                } else {
                    oneshot_receiver = None;
                }

                content = Some(current_content);
            } else {
                changed = false;
                content = None;
                oneshot_receiver = None;
            }
        };

        let new_local_entry = LocalEntry {
            kind,
            content: content.clone(),
        };
        if changed {
            if ctx.dry_run {
                info!("Would upload {}", local_path);
                ctx.ctx
                    .final_counters
                    .uploaded_entries
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                let item = AddVersionsTaskItem {
                    is_mount: ctx.is_mount,
                    version: AddVersion {
                        path: ctx.ctx.cipher.encrypt_path(archive_path)?,
                        record_trigger: RecordTrigger::Upload,
                        kind: Some(kind),
                        content: if let Some(content) = &content {
                            Some(FileContent {
                                modified_at: content.modified_at,
                                original_size: ctx
                                    .ctx
                                    .cipher
                                    .encrypt_size(content.original_size)?,
                                encrypted_size: content.encrypted_size,
                                hash: ctx.ctx.cipher.encrypt_content_hash(&content.hash)?,
                                unix_mode: content.unix_mode,
                            })
                        } else {
                            None
                        },
                    },
                    local_path: local_path.clone(),
                    local_entry_info: new_local_entry,
                };
                ctx.add_versions_sender
                    .send((item, oneshot_receiver))
                    .await
                    .map_err(|_| anyhow!("failed to send item to add version task"))?;
                ctx.ctx
                    .intermediate_counters
                    .queued_upload_entries
                    .fetch_add(1, Ordering::Relaxed);
            }
        } else if !ctx.dry_run {
            if let Some(new_content) = &new_local_entry.content {
                if let Some(old_content) = db_data.as_ref().and_then(|data| data.content.as_ref()) {
                    if new_content.modified_at != old_content.modified_at {
                        info!("updating modified_at in db for {}", local_path);
                        ctx.ctx.db.set_local_entry(local_path, &new_local_entry)?;
                    }
                }
            }
        }
        if is_dir {
            for entry in fs::read_dir(local_path)? {
                let entry = entry?;
                let entry_path = entry.path();
                if symlink_metadata(&entry_path)?.is_symlink() {
                    debug!("Skipping symlink: {:?}", entry_path);
                    continue;
                }
                let file_name = entry.file_name();
                let file_name_str = file_name
                    .to_str()
                    .ok_or_else(|| anyhow!("Unsupported file name: {:?}", entry_path))?;
                let entry_local_path = local_path.join(file_name_str)?;
                let entry_archive_path = archive_path.join_one(file_name_str).map_err(|err| {
                    anyhow!(
                        "Failed to construct archive path for {:?}: {:?}",
                        entry_path,
                        err
                    )
                })?;
                upload_inner(&mut *ctx, &entry_local_path, &entry_archive_path)
                    .await
                    .map_err(|err| anyhow!("Failed to process {:?}: {:?}", entry_path, err))?;
            }
        }
        Ok(())
    })
}

struct ContentUploadTaskItem {
    hash: ContentHash,
    local_path: SanitizedLocalPath,
    file_data: EncryptedFileHead,
    sender: oneshot::Sender<()>,
}

async fn content_upload_task(
    ctx: Arc<Ctx>,
    mut receiver: mpsc::Receiver<ContentUploadTaskItem>,
    error_sender: ErrorSender,
) {
    let semaphore = Arc::new(Semaphore::new(8));
    while let Some(item) = receiver.recv().await {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let ctx = ctx.clone();
        let error_sender = error_sender.clone();
        task::spawn(async move {
            let _permit = permit;
            let r = content_upload_item_task(ctx, item).await;
            error_sender.unwrap_or_notify(r).await;
        });
    }
}

async fn content_upload_item_task(ctx: Arc<Ctx>, item: ContentUploadTaskItem) -> Result<()> {
    let encrypted_hash = ctx.cipher.encrypt_content_hash(&item.hash)?;
    let exists = ctx
        .client
        .request(&ContentHashExists(encrypted_hash.clone()))
        .await?;
    if exists {
        let _ = item.sender.send(());
        return Ok(());
    }

    if item.file_data.encrypted_size > ctx.config.warn_about_files_larger_than.get_bytes() {
        warn!(
            "Uploading {} file: {}",
            pretty_size(item.file_data.encrypted_size),
            item.local_path
        );
    }

    ctx.client
        .upload(&encrypted_hash, item.file_data.file)
        .await?;
    ctx.final_counters
        .uploaded_bytes
        .fetch_add(item.file_data.encrypted_size, Ordering::SeqCst);
    let _ = item.sender.send(());
    Ok(())
}

struct AddVersionsTaskItem {
    is_mount: bool,
    version: AddVersion,
    local_path: SanitizedLocalPath,
    local_entry_info: LocalEntry,
}

async fn add_versions_task(
    ctx: Arc<Ctx>,
    mut receiver: mpsc::Receiver<(AddVersionsTaskItem, Option<oneshot::Receiver<()>>)>,
    error_sender: ErrorSender,
) {
    let r = async move {
        let mut versions = Vec::new();
        while let Some((item, receiver)) = receiver.recv().await {
            if let Some(receiver) = receiver {
                receiver.await?;
            }
            versions.push(item);
            if versions.len() >= BATCH_SIZE {
                add_versions_batch(&ctx, mem::take(&mut versions)).await?;
            }
        }
        add_versions_batch(&ctx, mem::take(&mut versions)).await?;
        anyhow::Ok(())
    }
    .await;
    error_sender.unwrap_or_notify(r).await;
}

async fn add_versions_batch(ctx: &Ctx, items: Vec<AddVersionsTaskItem>) -> Result<()> {
    let results = ctx
        .client
        .request(&AddVersions(
            items.iter().map(|item| item.version.clone()).collect(),
        ))
        .await?;

    if results.len() != items.len() {
        bail!("invalid item count in AddVersions response");
    }

    ctx.intermediate_counters
        .unqueued_upload_entries
        .fetch_add(items.len() as u64, Ordering::Relaxed);

    for (result, item) in results.into_iter().zip(items) {
        if result.added {
            ctx.final_counters
                .uploaded_entries
                .fetch_add(1, Ordering::Relaxed);
            info!("Uploaded {}", item.local_path);
        }
        if item.is_mount {
            ctx.db
                .set_local_entry(&item.local_path, &item.local_entry_info)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_special_file(file_type: &FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;

    file_type.is_block_device()
        || file_type.is_char_device()
        || file_type.is_fifo()
        || file_type.is_socket()
}

#[cfg(not(unix))]
fn is_special_file(_file_type: &FileType) -> bool {
    false
}
