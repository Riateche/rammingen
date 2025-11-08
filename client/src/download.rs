use {
    crate::{
        path::SanitizedLocalPath,
        rules::Rules,
        term::{set_status, set_status_updater},
        Ctx,
    },
    anyhow::{anyhow, bail, Result},
    fs_err::{create_dir, metadata, remove_dir, remove_file, rename},
    futures::{stream, Stream, TryStreamExt},
    rammingen_protocol::{
        endpoints::GetEntryVersionsAtTime,
        util::{archive_to_native_relative_path, interrupt_on_error, try_exists, ErrorSender},
        ArchivePath, DateTimeUtc, EntryKind,
    },
    rammingen_sdk::content::{DecryptedContentHead, DecryptedEntryVersion, LocalEntry},
    sha2::{Digest, Sha256},
    std::{
        path::Path,
        sync::{atomic::Ordering, Arc},
    },
    stream_generator::generate_try_stream,
    tokio::{
        sync::{mpsc, oneshot, Semaphore},
        task,
    },
    tracing::{info, warn},
};

fn archive_to_local_path(
    path: &ArchivePath,
    root_archive_path: &ArchivePath,
    root_local_path: &SanitizedLocalPath,
) -> Result<SanitizedLocalPath> {
    if path == root_archive_path {
        Ok(root_local_path.clone())
    } else {
        let relative_path = path
            .strip_prefix(root_archive_path)
            .ok_or_else(|| anyhow!("failed to strip path prefix from child"))?;
        root_local_path.join(&*archive_to_native_relative_path(relative_path))
    }
}

fn remove_dir_or_file(path: impl AsRef<Path>) -> Result<bool> {
    let path = path.as_ref();
    if fs_err::metadata(path)?.is_dir() {
        if let Err(err) = remove_dir(path) {
            warn!("Cannot remove directory {}: {}", path.display(), err);
            return Ok(false);
        }
    } else {
        remove_file(path)?;
    }
    Ok(true)
}

pub async fn download_version(
    ctx: &Arc<Ctx>,
    root_archive_path: &ArchivePath,
    root_local_path: &SanitizedLocalPath,
    version: DateTimeUtc,
) -> Result<bool> {
    let stream = generate_try_stream(move |mut y| async move {
        let mut response_stream = ctx.client.stream(&GetEntryVersionsAtTime {
            path: ctx.cipher.encrypt_path(root_archive_path)?,
            recorded_at: version,
        });
        let mut any = false;
        while let Some(entry) = response_stream.try_next().await? {
            let entry = DecryptedEntryVersion::new(entry.data, &ctx.cipher)?;
            any = true;
            y.send(Ok(entry)).await;
        }
        if !any {
            bail!("no such path: {}", root_archive_path);
        }
        Ok(())
    });
    download(
        ctx,
        root_archive_path,
        root_local_path,
        &mut Rules::new(&[&ctx.config.always_exclude], root_local_path.clone()),
        false,
        stream,
        false,
    )
    .await
}

pub async fn download_latest(
    ctx: &Arc<Ctx>,
    root_archive_path: &ArchivePath,
    root_local_path: &SanitizedLocalPath,
    rules: &mut Rules,
    is_mount: bool,
    dry_run: bool,
) -> Result<bool> {
    let data = stream::iter(ctx.db.get_archive_entries(root_archive_path));
    download(
        ctx,
        root_archive_path,
        root_local_path,
        rules,
        is_mount,
        data,
        dry_run,
    )
    .await
}

struct DownloadContext<'a> {
    ctx: &'a Arc<Ctx>,
    root_archive_path: &'a ArchivePath,
    root_local_path: &'a SanitizedLocalPath,
    rules: &'a mut Rules,
    is_mount: bool,
    dry_run: bool,
    file_download_sender: mpsc::Sender<DownloadFileTask>,
    finalize_sender: mpsc::Sender<FinalizeDownloadTaskItem>,
}

pub async fn download(
    ctx: &Arc<Ctx>,
    root_archive_path: &ArchivePath,
    root_local_path: &SanitizedLocalPath,
    rules: &mut Rules,
    is_mount: bool,
    versions: impl Stream<Item = Result<DecryptedEntryVersion>>,
    dry_run: bool,
) -> Result<bool> {
    interrupt_on_error(|error_sender| async move {
        let ctx2 = ctx.clone();
        let (file_download_sender, file_download_receiver) = mpsc::channel(100_000);

        let content_upload_task = task::spawn(download_files_task(
            ctx.clone(),
            file_download_receiver,
            error_sender.clone(),
        ));

        let (finalize_sender, finalize_receiver) = mpsc::channel(100_000);
        let versions_task = task::spawn(finalize_download_task(
            ctx.clone(),
            finalize_receiver,
            error_sender,
        ));

        let mut ctx = DownloadContext {
            ctx,
            root_archive_path,
            root_local_path,
            rules,
            is_mount,
            dry_run,
            file_download_sender,
            finalize_sender,
        };
        let r = download_inner(&mut ctx, versions).await?;
        drop(ctx);
        let _status = set_status_updater(move || {
            let queued = ctx2
                .intermediate_counters
                .queued_download_entries
                .load(Ordering::Relaxed);
            let unqueued = ctx2
                .final_counters
                .downloaded_entries
                .load(Ordering::Relaxed);
            format!("Downloading ({unqueued} / {queued} entries)")
        });
        content_upload_task.await?;
        versions_task.await?;
        Ok(r)
    })
    .await
}

async fn download_inner(
    ctx: &mut DownloadContext<'_>,
    versions: impl Stream<Item = Result<DecryptedEntryVersion>>,
) -> Result<bool> {
    tokio::pin!(versions);
    if ctx.is_mount {
        let _status = set_status("Checking for files deleted remotely");
        for entry in ctx.ctx.db.get_archive_entries(ctx.root_archive_path).rev() {
            let entry = entry?;
            if entry.kind.is_some() {
                continue;
            }
            let entry_local_path =
                archive_to_local_path(&entry.path, ctx.root_archive_path, ctx.root_local_path)?;
            if ctx.rules.matches(&entry_local_path)? {
                continue;
            }
            let Some(db_data) = ctx.ctx.db.get_local_entry(&entry_local_path)? else {
                continue;
            };
            if try_exists(entry_local_path.as_path())? {
                if ctx.dry_run {
                    info!("Would delete {}", entry_local_path);
                } else {
                    match db_data.kind {
                        EntryKind::File => {
                            remove_file(&entry_local_path)?;
                        }
                        EntryKind::Directory => {
                            if let Err(err) = remove_dir(&entry_local_path) {
                                warn!("Cannot remove directory {}: {}", entry_local_path, err);
                                continue;
                            }
                        }
                    }
                    info!("Deleted {}", entry_local_path);
                }
                ctx.ctx
                    .final_counters
                    .deleted_entries
                    .fetch_add(1, Ordering::SeqCst);
            }
            if !ctx.dry_run {
                ctx.ctx.db.remove_local_entry(&entry_local_path)?;
            }
        }
    }
    let mut found_any = false;
    while let Some(entry) = versions.try_next().await? {
        let Some(kind) = entry.kind else {
            continue;
        };
        let entry_local_path =
            archive_to_local_path(&entry.path, ctx.root_archive_path, ctx.root_local_path)?;
        if ctx.rules.matches(&entry_local_path)? {
            continue;
        }
        let _status = set_status(format!("Scanning remote files: {}", ctx.root_local_path));

        let mut must_delete = false;
        let db_data = if ctx.is_mount {
            ctx.ctx.db.get_local_entry(&entry_local_path)?
        } else {
            None
        };
        if let Some(db_data) = &db_data {
            if db_data.is_same_as_entry(&entry) {
                continue;
            }
            if !ctx.dry_run && !db_data.matches_real(&entry_local_path)? {
                bail!(
                    "local db data doesn't match local file at {:?}",
                    entry_local_path
                );
            }
            must_delete = true;
        }

        if ctx.dry_run {
            info!("Would download {}", entry_local_path);
            if let Some(content) = &entry.content {
                ctx.ctx
                    .final_counters
                    .downloaded_bytes
                    .fetch_add(content.encrypted_size, Ordering::SeqCst);
            }
        } else {
            let file_receiver;
            match kind {
                EntryKind::Directory => {
                    file_receiver = None;
                }
                EntryKind::File => {
                    let content = entry
                        .content
                        .clone()
                        .ok_or_else(|| anyhow!("missing content info for existing file"))?;
                    let (sender, receiver) = oneshot::channel();
                    file_receiver = Some(receiver);
                    let _ = ctx
                        .file_download_sender
                        .send(DownloadFileTask {
                            local_path: entry_local_path.clone(),
                            root_local_path: ctx.root_local_path.clone(),
                            content,
                            sender,
                        })
                        .await;
                }
            }
            let _ = ctx
                .finalize_sender
                .send(FinalizeDownloadTaskItem {
                    entry,
                    db_data,
                    local_path: entry_local_path,
                    must_delete,
                    file_receiver,
                })
                .await;
            ctx.ctx
                .intermediate_counters
                .queued_download_entries
                .fetch_add(1, Ordering::SeqCst);
        }

        found_any = true;
    }
    Ok(found_any)
}

struct TmpGuard(SanitizedLocalPath);

impl TmpGuard {
    fn path(&self) -> &SanitizedLocalPath {
        &self.0
    }
    fn clean(&mut self) -> Result<()> {
        if try_exists(&self.0)? {
            remove_file(&self.0)?;
        }
        Ok(())
    }
}

impl Drop for TmpGuard {
    fn drop(&mut self) {
        if let Err(err) = self.clean() {
            warn!(?err, "failed to clean up temporary file");
        }
    }
}

struct DownloadFileTask {
    local_path: SanitizedLocalPath,
    root_local_path: SanitizedLocalPath,
    content: DecryptedContentHead,
    sender: oneshot::Sender<TmpGuard>,
}

async fn download_files_task(
    ctx: Arc<Ctx>,
    mut receiver: mpsc::Receiver<DownloadFileTask>,
    error_sender: ErrorSender,
) {
    let semaphore = Arc::new(Semaphore::new(8));
    while let Some(item) = receiver.recv().await {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let ctx = ctx.clone();
        let error_sender = error_sender.clone();
        task::spawn(async move {
            let _permit = permit;
            let r = download_file_task(&ctx, item).await;
            error_sender.unwrap_or_notify(r).await;
        });
    }
}

async fn download_file_task(ctx: &Ctx, item: DownloadFileTask) -> Result<()> {
    let tmp_parent_dir = if metadata(&item.root_local_path).is_ok_and(|m| m.is_dir()) {
        item.root_local_path.clone()
    } else {
        item.root_local_path.parent()?.ok_or_else(|| {
            anyhow!(
                "failed to get tmp parent dir (root: {})",
                item.root_local_path
            )
        })?
    };
    let tmp_path =
        tmp_parent_dir.join(format!(".{}.rammingen.part", path_hash(&item.local_path)))?;
    let tmp_guard = TmpGuard(tmp_path.clone());
    if try_exists(&tmp_path)? {
        remove_file(&tmp_path)?;
    }
    ctx.client
        .download_and_decrypt(&item.content, &tmp_path, &ctx.cipher)
        .await?;
    let _ = item.sender.send(tmp_guard);
    Ok(())
}

fn path_hash(path: &SanitizedLocalPath) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_str());
    let hash = hasher.finalize();
    hex::encode(hash)
}

async fn finalize_download_task(
    ctx: Arc<Ctx>,
    mut receiver: mpsc::Receiver<FinalizeDownloadTaskItem>,
    error_sender: ErrorSender,
) {
    while let Some(item) = receiver.recv().await {
        let r = finalize_item_download(&ctx, item).await;
        error_sender.unwrap_or_notify(r).await;
    }
}

struct FinalizeDownloadTaskItem {
    entry: DecryptedEntryVersion,
    db_data: Option<LocalEntry>,
    local_path: SanitizedLocalPath,
    must_delete: bool,
    file_receiver: Option<oneshot::Receiver<TmpGuard>>,
}

async fn finalize_item_download(ctx: &Ctx, item: FinalizeDownloadTaskItem) -> Result<()> {
    if !item.must_delete && try_exists(&item.local_path)? {
        bail!(
            "local entry already exists at {:?} (while processing entry: {:?})",
            item.local_path,
            item.entry
        );
    }

    let kind = item
        .entry
        .kind
        .ok_or_else(|| anyhow!("missing kind in finalize_item_download"))?;
    match kind {
        EntryKind::Directory => {
            if let Some(db_data) = &item.db_data {
                // Check again just in case.
                if !db_data.matches_real(&item.local_path)? {
                    bail!(
                        "local db data doesn't match local file at {:?}",
                        item.local_path
                    );
                }
            }
            if item.must_delete {
                if !remove_dir_or_file(&item.local_path)? {
                    return Ok(());
                }
            }
            create_dir(&item.local_path)?;
            ctx.db.set_local_entry(
                &item.local_path,
                &LocalEntry {
                    kind,
                    content: None,
                },
            )?;
        }
        EntryKind::File => {
            let mut content = item
                .entry
                .content
                .ok_or_else(|| anyhow!("missing content info for existing file"))?;
            let file_receiver = item
                .file_receiver
                .ok_or_else(|| anyhow!("missing file_receiver for existing file"))?;
            let tmp_file = file_receiver.await?;
            if let Some(db_data) = &item.db_data {
                // Check again just in case.
                if !db_data.matches_real(&item.local_path)? {
                    bail!(
                        "local db data doesn't match local file at {:?}",
                        item.local_path
                    );
                }
            }
            if item.must_delete {
                if !remove_dir_or_file(&item.local_path)? {
                    return Ok(());
                }
            }
            rename(tmp_file.path(), &item.local_path)?;

            #[cfg(target_family = "unix")]
            {
                use std::{fs::Permissions, os::unix::prelude::PermissionsExt};

                if let Some(mode) = content.unix_mode {
                    fs_err::set_permissions(&item.local_path, Permissions::from_mode(mode))?;
                }
            }

            content.modified_at = fs_err::metadata(&item.local_path)?.modified()?.into();
            ctx.final_counters
                .downloaded_bytes
                .fetch_add(content.encrypted_size, Ordering::SeqCst);
            ctx.db.set_local_entry(
                &item.local_path,
                &LocalEntry {
                    kind,
                    content: Some(content),
                },
            )?;
        }
    }
    info!("Downloaded {}", item.local_path);
    ctx.final_counters
        .downloaded_entries
        .fetch_add(1, Ordering::SeqCst);
    Ok(())
}
