use std::cmp::max;

use anyhow::Result;
use futures::TryStreamExt;
use rammingen_protocol::GetEntries;

use crate::{db::DecryptedEntryVersionData, encryption::decrypt_path, term::set_status, Ctx};

pub async fn pull_updates(ctx: &Ctx) -> Result<()> {
    set_status("Pulling updates from server");
    let mut last_update_number = ctx.db.last_entry_update_number()?;
    let mut stream = ctx.client.stream(&GetEntries { last_update_number });
    let mut decrypted = Vec::new();
    while let Some(batch) = stream.try_next().await? {
        for update in batch {
            decrypted.push(DecryptedEntryVersionData {
                path: decrypt_path(&update.data.path, &ctx.cipher)?,
                recorded_at: update.data.recorded_at,
                source_id: update.data.source_id,
                record_trigger: update.data.record_trigger,
                kind: update.data.kind,
                content: update.data.content,
            });
            last_update_number = max(last_update_number, update.update_number);
        }
        ctx.db
            .update_archive_entries(&decrypted, last_update_number)?;
    }
    Ok(())
}
