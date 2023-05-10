use std::cmp::max;

use anyhow::Result;
use futures::TryStreamExt;
use rammingen_protocol::GetNewEntries;

use crate::{db::DecryptedEntryVersionData, term::set_status, Ctx};

pub async fn pull_updates(ctx: &Ctx) -> Result<()> {
    set_status("Pulling updates from server");
    let mut last_update_number = ctx.db.last_entry_update_number()?;
    let mut stream = ctx.client.stream(&GetNewEntries { last_update_number });
    let mut decrypted = Vec::new();
    while let Some(update) = stream.try_next().await? {
        decrypted.push(DecryptedEntryVersionData::new(ctx, update.data)?);
        last_update_number = max(last_update_number, update.update_number);
    }
    ctx.db
        .update_archive_entries(&decrypted, last_update_number)?;
    Ok(())
}
