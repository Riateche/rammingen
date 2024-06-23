#![allow(clippy::collapsible_if)]

pub mod cli;
pub mod config;
mod counters;
mod db;
mod download;
mod info;
pub mod path;
mod pull_updates;
pub mod rules;
mod sync;
pub mod term;
mod upload;

use crate::{
    info::{local_status, ls},
    pull_updates::pull_updates,
    upload::upload,
};
use anyhow::{anyhow, bail, Result};
use cli::Cli;
use config::Config;
use counters::Counters;
use derivative::Derivative;
use download::{download_latest, download_version};
use info::{list_versions, pretty_size};
use rammingen_protocol::{
    endpoints::{CheckIntegrity, GetServerStatus, MovePath, RemovePath, ResetVersion},
    util::log_writer,
};
use rules::Rules;
use std::fs::Metadata;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use sync::sync;
use term::TermLayer;
use tracing::info;
use tracing_subscriber::{
    prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

use rammingen_sdk::{client::Client, crypto::Cipher};

#[derive(Derivative)]
pub struct Ctx {
    pub config: Config,
    pub client: Client,
    #[derivative(Debug = "ignore")]
    pub cipher: Cipher,
    pub db: crate::db::Db,
    pub counters: Counters,
}

pub async fn run(cli: Cli, config: Config) -> Result<()> {
    let local_db_path = if let Some(v) = &config.local_db_path {
        v.clone()
    } else {
        let data_dir = dirs::data_dir().ok_or_else(|| anyhow!("cannot find config dir"))?;
        data_dir.join("rammingen.db")
    };
    let ctx = Arc::new(Ctx {
        client: Client::new(config.server_url.clone(), config.access_token.clone()),
        cipher: Cipher::new(&config.encryption_key),
        config,
        db: crate::db::Db::open(&local_db_path)?,
        counters: Counters::default(),
    });

    let dry_run = cli.command == cli::Command::DryRun;
    let result = handle_command(cli, &ctx).await;
    ctx.counters.report(dry_run, &ctx);
    result
}

async fn handle_command(cli: Cli, ctx: &Arc<Ctx>) -> Result<()> {
    match cli.command {
        cli::Command::DryRun => {
            sync(ctx, true).await?;
        }
        cli::Command::Sync => {
            sync(ctx, false).await?;
        }
        cli::Command::Upload {
            local_path,
            archive_path,
        } => {
            upload(
                ctx,
                &local_path,
                &archive_path,
                &mut Rules::new(&[&ctx.config.always_exclude], local_path.clone()),
                false,
                &mut HashSet::new(),
                false,
            )
            .await?;
        }
        cli::Command::Download {
            archive_path,
            local_path,
            version,
        } => {
            let found_any = if let Some(version) = version {
                download_version(ctx, &archive_path, &local_path, version.0).await?
            } else {
                pull_updates(ctx).await?;
                download_latest(
                    ctx,
                    &archive_path,
                    &local_path,
                    &mut Rules::new(&[&ctx.config.always_exclude], local_path.clone()),
                    false,
                    false,
                )
                .await?
            };
            if !found_any {
                bail!("no matching entries found");
            }
        }
        cli::Command::LocalStatus { path } => local_status(ctx, &path).await?,
        cli::Command::Ls { path, deleted } => ls(ctx, &path, deleted).await?,
        cli::Command::Reset {
            archive_path,
            version,
        } => {
            let stats = ctx
                .client
                .request(&ResetVersion {
                    path: ctx.cipher.encrypt_path(&archive_path)?,
                    recorded_at: version.into(),
                })
                .await?;
            info!("{:?}", stats);
        }
        cli::Command::Move { old_path, new_path } => {
            let stats = ctx
                .client
                .request(&MovePath {
                    old_path: ctx.cipher.encrypt_path(&old_path)?,
                    new_path: ctx.cipher.encrypt_path(&new_path)?,
                })
                .await?;
            info!("{stats:?}");
        }
        cli::Command::Remove { archive_path } => {
            let stats = ctx
                .client
                .request(&RemovePath {
                    path: ctx.cipher.encrypt_path(&archive_path)?,
                })
                .await?;
            info!("{:?}", stats);
        }
        cli::Command::History { path, recursive } => {
            list_versions(ctx, &path, recursive).await?;
        }
        cli::Command::Status => {
            let status = ctx.client.request(&GetServerStatus).await?;
            info!(
                "Available space on server: {}",
                pretty_size(status.available_space)
            );
        }
        cli::Command::CheckIntegrity => {
            ctx.client.request(&CheckIntegrity).await?;
            info!("It's fine.");
        }
        cli::Command::GenerateEncryptionKey => unreachable!(),
    }
    Ok(())
}

#[cfg(target_family = "unix")]
pub fn unix_mode(metadata: &Metadata) -> Option<u32> {
    use std::os::unix::prelude::PermissionsExt;

    Some(metadata.permissions().mode())
}

#[cfg(not(target_family = "unix"))]
pub fn unix_mode(_metadata: &Metadata) -> Option<u32> {
    None
}

pub fn setup_logger(log_file: Option<PathBuf>, log_filter: String) -> Result<()> {
    // Defaults to stdout if `data_dir()` fails.
    let log_file = log_file.or_else(|| dirs::data_dir().map(|dir| dir.join("rammingen.log")));
    let fmt_layer =
        tracing_subscriber::fmt::layer().with_writer(Mutex::new(log_writer(log_file.as_deref())?));
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(EnvFilter::try_new(log_filter)?)
        .with(TermLayer)
        .init();
    Ok(())
}
