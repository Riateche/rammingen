use std::{
    borrow::Cow,
    path::{MAIN_SEPARATOR, MAIN_SEPARATOR_STR},
};

use anyhow::{anyhow, bail, Result};
use fs_err::{create_dir, remove_dir, remove_file, rename};
use itertools::Itertools;
use rammingen_protocol::{util::try_exists, ArchivePath, EntryKind};

use crate::{
    db::LocalEntryInfo,
    path::SanitizedLocalPath,
    rules::Rules,
    term::{info, set_status},
    Ctx,
};

fn fix_path_separator(path: &str) -> Cow<'_, str> {
    if MAIN_SEPARATOR == '/' {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(path.split('/').join(MAIN_SEPARATOR_STR))
    }
}

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
        root_local_path.join(&*fix_path_separator(relative_path))
    }
}

pub async fn download(
    ctx: &Ctx,
    root_archive_path: &ArchivePath,
    root_local_path: &SanitizedLocalPath,
    rules: &mut Rules,
    is_mount: bool,
) -> Result<()> {
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
                        remove_dir(&entry_local_path)?;
                    }
                }
            }
            ctx.db.remove_local_entry(&entry_local_path)?;
            info(format!("Removed {}", entry_local_path));
        }
    }
    for entry in ctx.db.get_archive_entries(root_archive_path) {
        let entry = entry?;
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
            bail!("local entry already exists at {:?}", entry_local_path);
        }

        match kind {
            EntryKind::Directory => {
                if must_delete {
                    remove_dir(&entry_local_path)?;
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
                    .download(&content.hash, &tmp_path, &ctx.cipher)
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
                    remove_file(&entry_local_path)?;
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
        info(format!("Downloaded {}", entry_local_path));
    }
    Ok(())
}
