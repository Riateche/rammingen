use anyhow::Result;
use futures::TryStreamExt;
use rammingen_protocol::GetEntries;

use crate::{encryption::decrypt_path, term::set_status, Ctx};

pub async fn pull_updates(ctx: &Ctx) -> Result<()> {
    set_status("Pulling updates from server");
    let mut stream = ctx.client.stream(&GetEntries {
        last_update_number: ctx.db.last_entry_update_number()?,
    });
    while let Some(mut batch) = stream.try_next().await? {
        for update in &mut batch {
            update.data.path = decrypt_path(&update.data.path, &ctx.cipher)?;
        }
        ctx.db.update_archive_entries(&batch)?;
    }
    Ok(())
}
