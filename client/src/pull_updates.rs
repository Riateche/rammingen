use {
    crate::{term::set_status, Ctx},
    anyhow::Result,
    futures::TryStreamExt,
    rammingen_protocol::endpoints::GetNewEntries,
    rammingen_sdk::content::DecryptedEntryVersion,
    std::cmp::max,
};

pub async fn pull_updates(ctx: &Ctx) -> Result<()> {
    let _status = set_status("Pulling updates from server");
    let mut last_update_number = ctx.db.last_entry_update_number()?;
    let mut stream = ctx.client.stream(&GetNewEntries { last_update_number });
    let mut decrypted = Vec::new();
    while let Some(update) = stream.try_next().await? {
        decrypted.push(DecryptedEntryVersion::new(update.data, &ctx.cipher)?);
        last_update_number = max(last_update_number, update.update_number);
    }
    ctx.db
        .update_archive_entries(&decrypted, last_update_number)?;
    Ok(())
}
