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
use anyhow::{anyhow, bail, Context as _, Result};
use cli::{default_log_path, Cli};
use config::Config;
use counters::{FinalCounters, IntermediateCounters, NotificationCounters};
use derivative::Derivative;
use download::{download_latest, download_version};
use info::{list_versions, pretty_size};
use notify_rust::Notification;
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
use tracing::{info, warn};
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
    pub final_counters: FinalCounters,
    pub intermediate_counters: IntermediateCounters,
}

const KEYRING_SERVICE: &str = "rammingen";

enum SecretKind {
    AccessToken,
    EncryptionKey,
}

fn fetch_keyring_secret(kind: SecretKind) -> anyhow::Result<String> {
    let user = match kind {
        SecretKind::AccessToken => "rammingen_access_token",
        SecretKind::EncryptionKey => "rammingen_encryption_key",
    };

    let entry = keyring::Entry::new(KEYRING_SERVICE, user)?;
    match entry.get_password() {
        Ok(password) => Ok(password),
        Err(err) => {
            if matches!(err, keyring::Error::NoEntry) {
                info!("entry {user:?} not found in keyring");
                let prompt = match kind {
                    SecretKind::AccessToken => "Input access token: ",
                    SecretKind::EncryptionKey => "Input encryption key: ",
                };
                let value = rpassword::prompt_password(prompt)?;
                if value.is_empty() {
                    bail!("no value provided");
                }
                match entry.set_password(&value) {
                    Ok(()) => {
                        info!("entry {user:?} saved to keyring");
                    }
                    Err(err) => {
                        warn!("failed to save secret in keyring: {err}");
                    }
                }
                Ok(value)
            } else {
                Err(err.into())
            }
        }
    }
}

pub async fn run(cli: Cli, config: Config) -> Result<()> {
    let local_db_path = if let Some(v) = &config.local_db_path {
        v.clone()
    } else {
        let data_dir = dirs::data_dir().ok_or_else(|| anyhow!("cannot find config dir"))?;
        data_dir.join("rammingen.db")
    };

    if config.use_keyring && (config.encryption_key.is_some() || config.access_token.is_some()) {
        bail!(
            "invalid config: if `use_keyring` is true, \
            `encryption_key` and `access_token` cannot be specified in the config"
        );
    }

    let access_token = if config.use_keyring {
        fetch_keyring_secret(SecretKind::AccessToken)?.parse()?
    } else {
        config
            .access_token
            .clone()
            .context("missing `access_token` or `use_keyring` in config")?
    };

    let encryption_key = if config.use_keyring {
        fetch_keyring_secret(SecretKind::EncryptionKey)?.parse()?
    } else {
        config
            .encryption_key
            .clone()
            .context("missing `encryption_key` or `use_keyring` in config")?
    };

    let ctx = Arc::new(Ctx {
        client: Client::new(config.server_url.clone(), access_token),
        cipher: Cipher::new(&encryption_key),
        config,
        db: crate::db::Db::open(&local_db_path)?,
        final_counters: Default::default(),
        intermediate_counters: Default::default(),
    });

    let dry_run = cli.command == cli::Command::DryRun;
    let result = handle_command(cli, &ctx).await;
    info!(
        "{}",
        NotificationCounters::from(&ctx.final_counters).report(dry_run, false, &ctx)
    );
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
            info!("Integrity check complete, no issues found.");
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
    let log_file = log_file.or_else(|| {
        default_log_path()
            .inspect_err(|err| eprintln!("{err}"))
            .ok()
    });
    let fmt_layer =
        tracing_subscriber::fmt::layer().with_writer(Mutex::new(log_writer(log_file.as_deref())?));
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(EnvFilter::try_new(log_filter)?)
        .with(TermLayer)
        .init();
    Ok(())
}

pub fn show_notification(title: &str, text: &str) {
    #[cfg(target_os = "macos")]
    init_notifications();

    if let Err(err) = Notification::new().summary(title).body(text).show() {
        warn!("Failed to show notification: {err}");
    }
}

#[cfg(target_os = "macos")]
fn init_notifications() {
    use std::sync::Once;

    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if let Err(err) = notify_rust::set_application("com.rammingen.rammingen") {
            warn!("Failed to init notifications: {err}");
        }
    });
}
