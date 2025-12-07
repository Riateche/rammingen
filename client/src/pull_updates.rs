use {
    crate::{Ctx, term::set_status},
    anyhow::{Result, bail},
    futures::TryStreamExt,
    rammingen_protocol::endpoints::{GetNewEntries, GetServerStatus},
    rammingen_sdk::content::LocalArchiveEntry,
    std::cmp::max,
    tracing::info,
};

pub async fn pull_updates(ctx: &Ctx) -> Result<()> {
    let _status = set_status("Pulling updates from server");
    let server_id = ctx.client.request(&GetServerStatus).await?.server_id;
    if let Some(local_db_server_id) = ctx.db.server_id()? {
        if local_db_server_id != server_id {
            bail!(
                "Server ID changed! expected: {local_db_server_id:?}, actual: {server_id:?}\n\
                Run `rammingen clear-local-cache` to start using the new server"
            );
        }
    } else {
        info!("Recording server id: {server_id:?}");
        ctx.db.set_server_id(&server_id)?;
    }
    let mut last_update_number = ctx.db.last_entry_update_number()?;
    let mut stream = ctx.client.stream(&GetNewEntries { last_update_number });
    let mut decrypted = Vec::new();
    while let Some(update) = stream.try_next().await? {
        decrypted.push(LocalArchiveEntry::decrypt(update.data, &ctx.cipher)?);
        last_update_number = max(last_update_number, update.update_number);
    }
    ctx.db
        .update_archive_entries(&decrypted, last_update_number)?;
    Ok(())
}
