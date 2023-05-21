use std::path::Path;

use anyhow::{anyhow, bail, Result};
use fs_err::{create_dir, remove_dir, remove_file, rename};
use futures::{stream, Stream, TryStreamExt};
use rammingen_protocol::{
    endpoints::GetEntryVersionsAtTime,
    util::{archive_to_native_relative_path, try_exists},
    ArchivePath, DateTimeUtc, EntryKind,
};
use stream_generator::generate_try_stream;

use crate::{
    db::{DecryptedEntryVersionData, LocalEntryInfo},
    encryption::{encrypt_content_hash, encrypt_path},
    path::SanitizedLocalPath,
    rules::Rules,
    term::{info, set_status, warn},
    Ctx,
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
            warn(format!(
                "Cannot remove directory {}: {}",
                path.display(),
                err
            ));
            return Ok(false);
        }
    } else {
        remove_file(path)?;
    }
    Ok(true)
}

pub async fn download_version(
    ctx: &Ctx,
    root_archive_path: &ArchivePath,
    root_local_path: &SanitizedLocalPath,
    version: DateTimeUtc,
) -> Result<bool> {
    crate::term::debug("download_version");
    let stream = generate_try_stream(move |mut y| async move {
        let mut response_stream = ctx.client.stream(&GetEntryVersionsAtTime {
            path: encrypt_path(root_archive_path, &ctx.cipher)?,
            recorded_at: version,
        });
        let mut any = false;
        while let Some(entry) = response_stream.try_next().await? {
            let entry = DecryptedEntryVersionData::new(ctx, entry.data)?;
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
    )
    .await
}

pub async fn download_latest(
    ctx: &Ctx,
    root_archive_path: &ArchivePath,
    root_local_path: &SanitizedLocalPath,
    rules: &mut Rules,
    is_mount: bool,
) -> Result<bool> {
    let data = stream::iter(ctx.db.get_archive_entries(root_archive_path));
    download(
        ctx,
        root_archive_path,
        root_local_path,
        rules,
        is_mount,
        data,
    )
    .await
}

pub async fn download(
    ctx: &Ctx,
    root_archive_path: &ArchivePath,
    root_local_path: &SanitizedLocalPath,
    rules: &mut Rules,
    is_mount: bool,
    versions: impl Stream<Item = Result<DecryptedEntryVersionData>>,
) -> Result<bool> {
    tokio::pin!(versions);
    // TODO: better way to select tmp path?
    let tmp_path = root_local_path
        .parent()?
        .ok_or_else(|| anyhow!("failed to get parent for local path"))?
        .join("__rammingen_tmp")?;
    if is_mount {
        set_status("Checking for files deleted remotely");
        for entry in ctx.db.get_archive_entries(root_archive_path).rev() {
            let entry = entry?;
            if entry.kind.is_some() {
                continue;
            }
            let entry_local_path =
                archive_to_local_path(&entry.path, root_archive_path, root_local_path)?;
            if rules.matches(&entry_local_path)? {
                continue;
            }
            let Some(db_data) = ctx.db.get_local_entry(&entry_local_path)? else {
                continue;
            };
            if try_exists(entry_local_path.as_path())? {
                match db_data.kind {
                    EntryKind::File => {
                        remove_file(&entry_local_path)?;
                    }
                    EntryKind::Directory => {
                        if let Err(err) = remove_dir(&entry_local_path) {
                            warn(format!(
                                "Cannot remove directory {}: {}",
                                entry_local_path, err
                            ));
                            continue;
                        }
                    }
                }
            }
            ctx.db.remove_local_entry(&entry_local_path)?;
            info(format!("Removed {}", entry_local_path));
        }
    }
    let mut found_any = false;
    while let Some(entry) = versions.try_next().await? {
        let Some(kind) = entry.kind else {
            continue;
        };
        let entry_local_path =
            archive_to_local_path(&entry.path, root_archive_path, root_local_path)?;
        if rules.matches(&entry_local_path)? {
            continue;
        }
        set_status(format!("Scanning remote files: {}", root_local_path));

        let mut must_delete = false;
        let db_data = if is_mount {
            ctx.db.get_local_entry(&entry_local_path)?
        } else {
            None
        };
        if let Some(db_data) = &db_data {
            if db_data.is_same_as_entry(&entry) {
                continue;
            }
            if !db_data.matches_real(&entry_local_path)? {
                bail!(
                    "local db data doesn't match local file at {:?}",
                    entry_local_path
                );
            }
            must_delete = true;
        }
        if !must_delete && try_exists(entry_local_path.as_path())? {
            bail!(
                "local entry already exists at {:?} (while processing entry: {:?}",
                entry_local_path,
                entry
            );
        }

        match kind {
            EntryKind::Directory => {
                if must_delete {
                    if !remove_dir_or_file(&entry_local_path)? {
                        continue;
                    }
                }
                create_dir(&entry_local_path)?;
                ctx.db.set_local_entry(
                    &entry_local_path,
                    &LocalEntryInfo {
                        kind,
                        content: None,
                    },
                )?;
            }
            EntryKind::File => {
                let mut content = entry
                    .content
                    .ok_or_else(|| anyhow!("missing content info for existing file"))?;
                ctx.client
                    .download(
                        &encrypt_content_hash(&content.hash, &ctx.cipher)?,
                        &tmp_path,
                        &ctx.cipher,
                    )
                    .await?;
                if let Some(db_data) = &db_data {
                    // Check again just in case.
                    if !db_data.matches_real(&entry_local_path)? {
                        bail!(
                            "local db data doesn't match local file at {:?}",
                            entry_local_path
                        );
                    }
                }
                if must_delete {
                    if !remove_dir_or_file(&entry_local_path)? {
                        continue;
                    }
                }
                rename(&tmp_path, &entry_local_path)?;

                #[cfg(target_family = "unix")]
                {
                    use std::fs::Permissions;
                    use std::os::unix::prelude::PermissionsExt;

                    if let Some(mode) = content.unix_mode {
                        fs_err::set_permissions(&entry_local_path, Permissions::from_mode(mode))?;
                    }
                }

                content.modified_at = fs_err::metadata(&entry_local_path)?.modified()?.into();
                ctx.db.set_local_entry(
                    &entry_local_path,
                    &LocalEntryInfo {
                        kind,
                        content: Some(content),
                    },
                )?;
            }
        }
        found_any = true;
        info(format!("Downloaded {}", entry_local_path));
    }
    Ok(found_any)
}
