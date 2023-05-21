use std::fmt::Display;

use anyhow::{anyhow, Result};
use byte_unit::Byte;
use futures::TryStreamExt;
use itertools::Itertools;
use prettytable::{cell, format::FormatBuilder, row, Table};
use rammingen_protocol::{
    endpoints::{GetAllEntryVersions, GetDirectChildEntries},
    util::local_time,
    ArchivePath, EntryKind,
};

use crate::{
    db::DecryptedEntryVersionData,
    encryption::encrypt_path,
    path::SanitizedLocalPath,
    pull_updates::pull_updates,
    rules::Rules,
    term::{error, info},
    upload::to_archive_path,
    Ctx,
};

pub async fn local_status(ctx: &Ctx, path: &SanitizedLocalPath) -> Result<()> {
    pull_updates(ctx).await?;
    let mut mount_points = ctx
        .config
        .mount_points
        .iter()
        .map(|mount_point| {
            let rules = Rules::new(
                &[&ctx.config.always_exclude, &mount_point.exclude],
                mount_point.local_path.clone(),
            );
            (mount_point, rules)
        })
        .collect_vec();

    info(format!("normalized local path: {}", path));

    if let Some((archive_path, rules)) = to_archive_path(path, &mut mount_points)? {
        if rules.matches(path)? {
            info("this path is ignored according to the configured exclude rules");
        } else {
            info(format!("archive path: {}", archive_path));
            let encrypted = encrypt_path(&archive_path, &ctx.cipher)?;
            info(format!("encrypted archive path: {}", encrypted));
            info(format!(
                "archive entry in local db: {:?}",
                ctx.db.get_archive_entry(&archive_path)?
            ));
            info(format!(
                "local entry in local db: {:?}",
                ctx.db.get_local_entry(path)?
            ));
        }
    } else {
        info("this path is not inside any of the configured mount points");
    }

    Ok(())
}

pub async fn ls(ctx: &Ctx, path: &ArchivePath, show_deleted: bool) -> Result<()> {
    pull_updates(ctx).await?;

    let Some(main_entry) = ctx.db.get_archive_entry(path)? else {
        error("no such path");
        return Ok(());
    };

    info(format!("path: {}", main_entry.path));
    info(format!(
        "recorded at: {}",
        local_time(main_entry.recorded_at)
    ));
    info(format!("source id: {}", main_entry.source_id.0));
    info(format!("record trigger: {:?}", main_entry.record_trigger));
    if let Some(kind) = main_entry.kind {
        match kind {
            EntryKind::File => {
                info("current status: existing file");
                let content = main_entry
                    .content
                    .ok_or_else(|| anyhow!("missing content for file entry"))?;
                info(format!(
                    "FS modified at: {}",
                    local_time(content.modified_at)
                ));
                info(format!(
                    "original size: {} ({} bytes)",
                    pretty_size(content.original_size),
                    content.original_size
                ));
                info(format!(
                    "encrypted size: {} ({} bytes)",
                    pretty_size(content.encrypted_size),
                    content.encrypted_size
                ));
                if let Some(unix_mode) = content.unix_mode {
                    info(format!("unix mode: {:#o}", unix_mode));
                } else {
                    info("unix mode: n/a");
                }
                info(format!("content hash: {}", content.hash));
            }
            EntryKind::Directory => {
                info("current status: existing directory");
            }
        }
    } else {
        info("current status: deleted");
    }

    let mut entries = Vec::new();
    let mut stream = ctx
        .client
        .stream(&GetDirectChildEntries(encrypt_path(path, &ctx.cipher)?));

    while let Some(entry) = stream.try_next().await? {
        entries.push(DecryptedEntryVersionData::new(ctx, entry.data)?);
    }
    // already sorted by path, so we use stable sort
    entries.sort_by_key(|entry| match &entry.kind {
        Some(EntryKind::Directory) => 0,
        Some(EntryKind::File) => 1,
        None => 2,
    });

    if !entries.is_empty() {
        info("");
    }
    let mut table = Table::new();
    table.set_format(FormatBuilder::new().column_separator(' ').build());
    let mut num_hidden_deleted = 0;
    for entry in entries {
        let name = entry
            .path
            .last_name()
            .ok_or_else(|| anyhow!("any child entry must have last name (path: {}", entry.path))?;
        let recorded_at = local_time(entry.recorded_at).format("%Y/%m/%d %H:%M:%S");
        if entry.kind.is_none() && !show_deleted {
            num_hidden_deleted += 1;
            continue;
        }
        let status = pretty_status(&entry)?;
        table.add_row(row![recorded_at, status, name]);
    }
    info(table);

    if num_hidden_deleted > 0 {
        info(format!(
            "{} deleted entries (use --deleted to view)",
            num_hidden_deleted
        ));
    }

    Ok(())
}

fn pretty_status(data: &DecryptedEntryVersionData) -> Result<String> {
    let text = if let Some(kind) = data.kind {
        match kind {
            EntryKind::File => {
                let content = data
                    .content
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing content for file entry"))?;
                let mode = if let Some(unix_mode) = content.unix_mode {
                    format!("{:o}", unix_mode & 0o777)
                } else {
                    "FILE".into()
                };
                format!("{} {}", mode, pretty_size(content.original_size))
            }
            EntryKind::Directory => "DIR".to_string(),
        }
    } else {
        "DEL".to_string()
    };
    Ok(text)
}

pub fn pretty_size(size: u64) -> impl Display {
    Byte::from_bytes(size.into())
        .get_appropriate_unit(false)
        .to_string()
}

pub async fn list_versions(ctx: &Ctx, path: &ArchivePath, recursive: bool) -> Result<()> {
    let mut stream = ctx.client.stream(&GetAllEntryVersions {
        path: encrypt_path(path, &ctx.cipher)?,
        recursive,
    });
    let mut table = Table::new();
    let parent = path.parent();
    table.set_format(FormatBuilder::new().column_separator(' ').build());
    let mut header = row!["Recorded", "Status", "Trigger", "Source"];
    if recursive {
        header.add_cell(cell!("Path"));
    }
    table.add_row(header);
    while let Some(item) = stream.try_next().await? {
        let data = DecryptedEntryVersionData::new(ctx, item.data)?;
        let recorded_at = local_time(data.recorded_at).format("%Y/%m/%d %H:%M:%S");
        let status = pretty_status(&data)?;
        let trigger = format!("{:?}", data.record_trigger);
        let mut row = row![recorded_at, status, trigger, data.source_id.0];
        if recursive {
            let relative_path = if let Some(parent) = &parent {
                data.path
                    .strip_prefix(parent)
                    .ok_or_else(|| anyhow!("strip_prefix({:?}, {:?}) failed", data.path, parent))?
                    .to_string()
            } else {
                data.path.to_str_without_prefix().to_string()
            };
            row.add_cell(cell!(relative_path));
        }
        table.add_row(row);
        if table.len() > 50 {
            info(table);
            table = Table::new();
            table.set_format(FormatBuilder::new().column_separator(' ').build());
        }
    }
    info(table);
    Ok(())
}
