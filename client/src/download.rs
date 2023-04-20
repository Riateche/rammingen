use anyhow::{anyhow, Result};
use fs_err::{create_dir, rename};
use itertools::Itertools;
use rammingen_protocol::{ArchivePath, EntryKind};
use std::{
    borrow::Cow,
    path::{Path, MAIN_SEPARATOR, MAIN_SEPARATOR_STR},
};

use crate::Ctx;

pub async fn download<'a>(
    ctx: &'a Ctx,
    archive_path: &'a ArchivePath,
    local_path: &'a Path,
) -> Result<()> {
    let tmp_path = local_path
        .parent()
        .ok_or_else(|| anyhow!("failed to get parent for local path"))?
        .join("__rammingen_tmp");
    for entry in ctx.db.get_archive_entries(archive_path) {
        let entry = entry?;
        let Some(kind) = entry.kind else {
            continue;
        };
        let entry_local_path = if &entry.path == archive_path {
            local_path.to_path_buf()
        } else {
            let prefix = entry
                .path
                .strip_prefix(archive_path)
                .expect("failed to strip path prefix from child");
            local_path.join(&*fix_path_separator(prefix))
        };

        match kind {
            EntryKind::Directory => {
                create_dir(&entry_local_path)?;
            }
            EntryKind::File => {
                let content = entry
                    .content
                    .ok_or_else(|| anyhow!("missing content info for existing file"))?;
                ctx.client
                    .download(&content.hash, &tmp_path, &ctx.cipher)
                    .await?;
                rename(&tmp_path, &entry_local_path)?;

                #[cfg(target_family = "unix")]
                {
                    use std::fs::Permissions;
                    use std::os::unix::prelude::PermissionsExt;

                    if let Some(mode) = content.unix_mode {
                        fs_err::set_permissions(&entry_local_path, Permissions::from_mode(mode))?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn fix_path_separator(path: &str) -> Cow<'_, str> {
    if MAIN_SEPARATOR == '/' {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(path.split('/').join(MAIN_SEPARATOR_STR))
    }
}
