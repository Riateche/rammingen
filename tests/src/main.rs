mod diff;
mod shuffle;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Result};
use chrono::{DateTime, FixedOffset, Utc};
use clap::{Parser, Subcommand};
use diff::{diff, diff_ignored, is_leftover_dir_with_ignored_files};
use fs_err::{
    copy, create_dir, create_dir_all, read_dir, remove_dir_all, remove_file, rename, write,
};
use futures::future::pending;
use portpicker::pick_unused_port;
use rammingen_server::util::{add_source, migrate};
use rand::{seq::SliceRandom, thread_rng, Rng, SeedableRng};
use rand_chacha::ChaCha12Rng;
use reqwest::Url;
use shuffle::{choose_path, random_content, random_name, shuffle};
use sqlx::PgPool;
use tempfile::TempDir;
use tokio::time::{interval, sleep};
use tracing::{debug, error, info};

use rammingen::{
    config::MountPoint, path::SanitizedLocalPath, rules::Rule, setup_logger, term::clear_status,
};
use rammingen_protocol::{
    credentials::{AccessToken, EncryptionKey},
    util::native_to_archive_relative_path,
    ArchivePath, DateTimeUtc,
};

fn copy_dir_all(src: &Path, dst: impl AsRef<Path>) -> Result<()> {
    create_dir_all(&dst)?;
    for entry in read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs_err::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(err) = try_main().await {
        error!("{:?}", err);
    }
}

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(long)]
    pub database_url: Option<String>,
    #[clap(long)]
    pub bind_addr: Option<SocketAddr>,
    #[clap(long)]
    pub server_url: Option<Url>,
    #[clap(long)]
    seed: Option<u64>,
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    Shuffle,
    Snapshot,
    ServerOnly,
}

async fn try_main() -> Result<()> {
    // TODO: remove into_path
    let dir = TempDir::new()?.into_path();
    let cli = Cli::parse();

    setup_logger(
        Some(dir.join("1.log")),
        "info,sqlx=warn,rammingen_server=debug".into(),
    )?;

    let server_url = if let Some(database_url) = cli.database_url {
        let db_pool = PgPool::connect(&database_url).await?;
        migrate(&db_pool).await?;

        debug!("dir: {}", dir.display());
        let storage_path = dir.join("storage");
        create_dir_all(&storage_path)?;

        let bind_addr = if let Some(bind_addr) = cli.bind_addr {
            bind_addr
        } else {
            let port = pick_unused_port().expect("failed to pick port");
            SocketAddr::new("127.0.0.1".parse()?, port)
        };
        let server_config = rammingen_server::Config {
            bind_addr,
            database_url: database_url.clone(),
            storage_path,
            log_file: None,
            log_filter: String::new(),
            retain_detailed_history_for: match &cli.command {
                Command::Shuffle | Command::ServerOnly => Duration::from_secs(3600),
                Command::Snapshot => Duration::from_secs(10),
            },
            snapshot_interval: match &cli.command {
                Command::Shuffle | Command::ServerOnly => Duration::from_secs(3600),
                Command::Snapshot => Duration::from_secs(5),
            },
        };
        write(
            dir.join("rammingen-server.conf"),
            json5::to_string(&server_config)?,
        )?;
        for client_index in 0..3 {
            add_source(
                &db_pool,
                &format!("client{client_index}"),
                &access_token(client_index),
            )
            .await?;
        }
        tokio::spawn(async move {
            if let Err(err) = rammingen_server::run(server_config).await {
                clear_status();
                error!("server failed: {err:?}");
                std::process::exit(1);
            }
        });
        format!("http://{bind_addr}/").parse()?
    } else if let Some(server_url) = cli.server_url {
        server_url
    } else {
        bail!("required to specify either database_url or server_url");
    };

    let encryption_key = EncryptionKey::generate();
    let mut clients = Vec::new();
    let archive_mount_path: ArchivePath = "ar:/my_files".parse()?;
    for client_index in 0..3 {
        let client_dir = dir.join(format!("client{client_index}"));
        let mount_dir = client_dir.join("mount1");
        create_dir_all(&mount_dir)?;
        let config = rammingen::config::Config {
            use_keyring: false,
            always_exclude: vec![
                Rule::NameEquals("target".into()),
                Rule::NameMatches("^build_".parse()?),
            ],
            mount_points: vec![MountPoint {
                local_path: mount_dir.to_str().unwrap().parse()?,
                archive_path: archive_mount_path.clone(),
                exclude: vec![],
            }],
            encryption_key: Some(encryption_key.clone()),
            server_url: server_url.clone(),
            access_token: Some(access_token(client_index)),
            local_db_path: Some(client_dir.join("db")),
            log_file: None,
            log_filter: String::new(),
            warn_about_files_larger_than: "50 MB".parse().unwrap(),
            enable_desktop_notifications: false,
            desktop_notification_interval: Default::default(),
        };
        let config_path = client_dir.join("rammingen.conf");
        write(&config_path, json5::to_string(&config)?)?;
        clients.push(ClientData { config, mount_dir });
    }

    let ctx = Context {
        clients,
        dir,
        archive_mount_path,
    };
    let seed = cli.seed.unwrap_or_else(|| {
        let v: u64 = thread_rng().gen();
        info!("No seed provided, choosing random seed = {}", v);
        v
    });
    let mut rng = ChaCha12Rng::seed_from_u64(seed);
    match cli.command {
        Command::Shuffle => test_shuffle(ctx, &mut rng).await,
        Command::Snapshot => test_snapshot(ctx, &mut rng).await,
        Command::ServerOnly => {
            info!("Started server at {server_url}");
            pending().await
        }
    }
}

fn access_token(index: usize) -> AccessToken {
    format!("accesstoken{index:0>53}").parse().unwrap()
}

struct Context {
    clients: Vec<ClientData>,
    dir: PathBuf,
    archive_mount_path: ArchivePath,
}

async fn test_shuffle(ctx: Context, rng: &mut impl Rng) -> Result<()> {
    let old_snapshot_path = ctx.dir.join("old_snapshot");
    let mut snapshot_time: Option<DateTime<Utc>> = None;
    let steps = 100;
    'outer: for step in 1..=steps {
        info!("Step {step} / {steps}");
        if rng.gen_bool(0.2) {
            // mutate through server command
            let expected = ctx.dir.join("expected");
            if expected.exists() {
                remove_dir_all_or_file(&expected)?;
            }
            copy_dir_all(&ctx.clients[0].mount_dir, &expected)?;
            let client1 = ctx.clients.choose(rng).unwrap();
            match rng.gen_range(0..=4) {
                0 => {
                    // reset
                    let Some(snapshot_time_value) = snapshot_time else {
                        continue;
                    };
                    let local_path =
                        choose_path(&old_snapshot_path, true, true, true, false, rng)?.unwrap();
                    if is_leftover_dir_with_ignored_files(&local_path)? {
                        continue;
                    }
                    let archive_path =
                        archive_subpath(&ctx.archive_mount_path, &old_snapshot_path, &local_path)?;
                    let path_in_expected = if local_path == old_snapshot_path {
                        expected.clone()
                    } else {
                        expected.join(local_path.strip_prefix(&old_snapshot_path)?)
                    };
                    if path_in_expected.exists() {
                        remove_dir_all_or_file(&path_in_expected)?;
                    }
                    let parent_path_in_expected = path_in_expected.parent().unwrap();
                    if !parent_path_in_expected.exists() {
                        create_dir_all(parent_path_in_expected)?;
                    }
                    if local_path.is_file() {
                        copy(&local_path, &path_in_expected)?;
                    } else {
                        copy_dir_all(&local_path, &path_in_expected)?;
                    }
                    info!(
                        "Checking reset: {}, {:?}",
                        archive_path, snapshot_time_value
                    );
                    client1
                        .reset(archive_path, snapshot_time_value.into())
                        .await?;
                    snapshot_time = None;
                }
                1 => {
                    // upload new path
                    let path_for_upload = ctx.dir.join("for_upload");
                    if path_for_upload.exists() {
                        remove_dir_all_or_file(&path_for_upload)?;
                    }
                    if rng.gen_bool(0.3) {
                        write(&path_for_upload, random_content(rng))?;
                    } else {
                        create_dir(&path_for_upload)?;
                        shuffle(&path_for_upload, rng)?;
                    }
                    let parent_path =
                        choose_path(&expected, false, true, true, false, rng)?.unwrap();
                    let path_in_expected = parent_path.join(random_name(false, rng));
                    if path_in_expected.exists() {
                        continue;
                    }
                    if path_for_upload.is_dir() {
                        copy_dir_all(&path_for_upload, &path_in_expected)?;
                    } else {
                        copy(&path_for_upload, &path_in_expected)?;
                    }
                    let archive_path =
                        archive_subpath(&ctx.archive_mount_path, &expected, &path_in_expected)?;
                    debug!("Checking upload ({archive_path})");
                    client1
                        .upload(SanitizedLocalPath::new(&path_for_upload)?, archive_path)
                        .await?;
                }
                2 => {
                    // move path
                    let Some(path1) = choose_path(&expected, true, true, false, false, rng)? else {
                        continue;
                    };
                    let path2_parent =
                        choose_path(&expected, false, true, true, false, rng)?.unwrap();
                    let path2 = path2_parent.join(random_name(false, rng));
                    if path2.exists() || path2.starts_with(&path1) {
                        continue;
                    }
                    rename(&path1, &path2)?;
                    let archive_path = archive_subpath(&ctx.archive_mount_path, &expected, &path1)?;
                    let new_archive_path =
                        archive_subpath(&ctx.archive_mount_path, &expected, &path2)?;
                    debug!("Checking mv ({archive_path} -> {new_archive_path})");
                    client1.move_path(archive_path, new_archive_path).await?;
                }
                3 => {
                    // remove path
                    let Some(path1) = choose_path(&expected, true, true, false, false, rng)? else {
                        continue;
                    };
                    if is_leftover_dir_with_ignored_files(&path1)? {
                        continue;
                    }
                    remove_dir_all_or_file(&path1)?;
                    let archive_path = archive_subpath(&ctx.archive_mount_path, &expected, &path1)?;
                    debug!("Checking rm {archive_path}");
                    client1.remove_path(archive_path).await?;
                }
                4 => {
                    // simultaneous edit of two mounts
                    let two_clients: Vec<_> = ctx.clients.choose_multiple(rng, 2).collect();
                    let mut chosen_paths = Vec::<(PathBuf, PathBuf)>::new();
                    info!("Checking simultaneous edit");
                    for client in &two_clients {
                        let Some(path1) =
                            choose_path(&client.mount_dir, true, true, false, false, rng)?
                        else {
                            continue;
                        };
                        if is_leftover_dir_with_ignored_files(&path1)? {
                            continue;
                        }
                        let path_in_expected =
                            expected.join(path1.strip_prefix(&client.mount_dir)?);
                        if path_in_expected.exists() {
                            remove_dir_all_or_file(&path_in_expected)?;
                        }
                        let parent_path_in_expected = path_in_expected.parent().unwrap();
                        if !parent_path_in_expected.exists() {
                            create_dir_all(parent_path_in_expected)?;
                        }
                        for (_, prev) in &chosen_paths {
                            if prev == &path_in_expected
                                || prev.starts_with(&path_in_expected)
                                || path_in_expected.starts_with(prev)
                            {
                                continue 'outer;
                            }
                        }
                        chosen_paths.push((path1, path_in_expected));
                    }
                    for (path1, path_in_expected) in &chosen_paths {
                        info!("Shuffling {}", path1.display());
                        if path1.is_dir() {
                            shuffle(path1, rng)?;
                        } else {
                            write(path1, random_content(rng))?;
                        }
                        if path1.is_file() {
                            copy(path1, path_in_expected)?;
                        } else {
                            copy_dir_all(path1, path_in_expected)?;
                        }
                    }
                    for client in &two_clients {
                        client.sync().await?;
                    }
                }
                _ => unreachable!(),
            }
            for client in &ctx.clients {
                client.sync().await?;
                diff(&expected, &client.mount_dir)?;
            }
        } else {
            // edit mount
            let index = rng.gen_range(0..ctx.clients.len());
            for _ in 0..rng.gen_range(1..=3) {
                debug!("Shuffling mount for client {index}");
                shuffle(&ctx.clients[index].mount_dir, rng)?;
                debug!("Syncing client {index}");
                ctx.clients[index].sync().await?;
            }
            for (index2, client) in ctx.clients.iter().enumerate() {
                if index2 != index {
                    debug!("Syncing client {index2}");
                    let before_sync_snapshot = ctx.dir.join("snapshot");
                    if before_sync_snapshot.exists() {
                        remove_dir_all(&before_sync_snapshot)?;
                    }
                    copy_dir_all(&client.mount_dir, &before_sync_snapshot)?;

                    if rng.gen_bool(0.2) {
                        client.dry_run().await?;
                        diff(&client.mount_dir, &before_sync_snapshot)?;
                    }

                    client.sync().await?;
                    diff_ignored(&client.mount_dir, &before_sync_snapshot)?;
                }
            }
            for client in &ctx.clients[1..] {
                diff(&ctx.clients[0].mount_dir, &client.mount_dir)?;
            }
        }
        check_download(
            &ctx.dir,
            &ctx.archive_mount_path,
            &ctx.clients,
            None,
            &ctx.clients.choose(rng).unwrap().mount_dir,
            rng,
        )
        .await?;
        if rng.gen_bool(0.3) {
            if let Some(snapshot_time_value) = snapshot_time {
                check_download(
                    &ctx.dir,
                    &ctx.archive_mount_path,
                    &ctx.clients,
                    Some(snapshot_time_value),
                    &old_snapshot_path,
                    rng,
                )
                .await?;
                snapshot_time = None;
            } else {
                sleep(Duration::from_millis(500)).await;
                snapshot_time = Some(Utc::now());
                info!("Saving snapshot for later ({snapshot_time:?})");
                if old_snapshot_path.exists() {
                    remove_dir_all_or_file(&old_snapshot_path)?;
                }
                copy_dir_all(&ctx.clients[0].mount_dir, &old_snapshot_path)?;
                sleep(Duration::from_millis(500)).await;
            }
        }
        ctx.clients[0].check_integrity().await?;
    }
    Ok(())
}

async fn test_snapshot(ctx: Context, rng: &mut impl Rng) -> Result<()> {
    let index = 0;
    let mut snapshots = Vec::<(PathBuf, DateTimeUtc)>::new();
    let mut interval = interval(Duration::from_secs(1));
    for i in 0..30 {
        interval.tick().await;
        debug!("Shuffling mount for client {index}");
        while snapshots
            .iter()
            .any(|(path, _)| diff(path, &ctx.clients[index].mount_dir).is_ok())
        {
            shuffle(&ctx.clients[index].mount_dir, rng)?;
        }
        debug!("Syncing client {index}");
        ctx.clients[index].sync().await?;
        let snapshot_path = ctx.dir.join(format!("snapshot_{i}"));
        debug!("Recording snapshot {i}");
        copy_dir_all(&ctx.clients[index].mount_dir, &snapshot_path)?;
        snapshots.push((snapshot_path, Utc::now()));
        ctx.clients[0].check_integrity().await?;
    }
    let download_path = ctx.dir.join("download");
    let mut results = Vec::new();
    for (i, (_, time)) in snapshots.iter().enumerate() {
        if download_path.exists() {
            remove_dir_all_or_file(&download_path)?;
        }
        match ctx.clients[index]
            .download(
                ctx.archive_mount_path.clone(),
                download_path.to_str().unwrap().parse()?,
                Some(*time),
            )
            .await
        {
            Ok(()) => {
                let mut same_as = Vec::new();
                for (i2, (path, time2)) in snapshots.iter().enumerate() {
                    if diff(&download_path, path).is_ok() {
                        info!("Download {i} ({time}) is the same as snapshot {i2} ({time2})");
                        same_as.push(i2);
                    }
                }
                if same_as.len() != 1 {
                    bail!("expected result to be the same as exactly one snapshot");
                }
                results.push(Some(same_as[0]));
            }
            Err(err) => {
                debug!("Cannot download {i} ({time}): {err:?}");
                results.push(None);
            }
        }
    }
    // Expected snapshots: after i = 4, 9, 14.
    assert_eq!(
        results,
        vec![
            // No info because the first snapshot (after i = 4) removes all previous versions.
            None,
            None,
            None,
            None,
            None,
            Some(4),
            Some(4),
            Some(4),
            Some(4),
            Some(4),
            Some(9),
            Some(9),
            Some(9),
            Some(9),
            Some(9),
            Some(15),
            Some(16),
            Some(17),
            Some(18),
            Some(19),
            Some(20),
            Some(21),
            Some(22),
            Some(23),
            Some(24),
            Some(25),
            Some(26),
            Some(27),
            Some(28),
            Some(29),
        ]
    );
    Ok(())
}

struct ClientData {
    mount_dir: PathBuf,
    config: rammingen::config::Config,
}

impl ClientData {
    async fn sync(&self) -> Result<()> {
        rammingen::run(
            rammingen::cli::Cli {
                config: None,
                command: rammingen::cli::Command::Sync,
            },
            self.config.clone(),
        )
        .await
    }

    async fn dry_run(&self) -> Result<()> {
        rammingen::run(
            rammingen::cli::Cli {
                config: None,
                command: rammingen::cli::Command::DryRun,
            },
            self.config.clone(),
        )
        .await
    }

    async fn download(
        &self,
        archive_path: ArchivePath,
        local_path: SanitizedLocalPath,
        version: Option<DateTimeUtc>,
    ) -> Result<()> {
        rammingen::run(
            rammingen::cli::Cli {
                config: None,
                command: rammingen::cli::Command::Download {
                    archive_path,
                    local_path,
                    version: version.map(Into::into),
                },
            },
            self.config.clone(),
        )
        .await
    }

    async fn upload(
        &self,
        local_path: SanitizedLocalPath,
        archive_path: ArchivePath,
    ) -> Result<()> {
        rammingen::run(
            rammingen::cli::Cli {
                config: None,
                command: rammingen::cli::Command::Upload {
                    local_path,
                    archive_path,
                },
            },
            self.config.clone(),
        )
        .await
    }

    async fn move_path(
        &self,
        archive_path: ArchivePath,
        new_archive_path: ArchivePath,
    ) -> Result<()> {
        rammingen::run(
            rammingen::cli::Cli {
                config: None,
                command: rammingen::cli::Command::Move {
                    old_path: archive_path,
                    new_path: new_archive_path,
                },
            },
            self.config.clone(),
        )
        .await
    }

    async fn remove_path(&self, archive_path: ArchivePath) -> Result<()> {
        rammingen::run(
            rammingen::cli::Cli {
                config: None,
                command: rammingen::cli::Command::Remove { archive_path },
            },
            self.config.clone(),
        )
        .await
    }

    async fn reset(&self, archive_path: ArchivePath, version: DateTime<FixedOffset>) -> Result<()> {
        rammingen::run(
            rammingen::cli::Cli {
                config: None,
                command: rammingen::cli::Command::Reset {
                    archive_path,
                    version,
                },
            },
            self.config.clone(),
        )
        .await
    }

    async fn check_integrity(&self) -> Result<()> {
        rammingen::run(
            rammingen::cli::Cli {
                config: None,
                command: rammingen::cli::Command::CheckIntegrity,
            },
            self.config.clone(),
        )
        .await
    }
}

fn archive_subpath(
    archive_root_path: &ArchivePath,
    local_root_path: &Path,
    path: &Path,
) -> Result<ArchivePath> {
    if path == local_root_path {
        Ok(archive_root_path.clone())
    } else {
        let relative = path.strip_prefix(local_root_path)?;
        archive_root_path.join_multiple(&native_to_archive_relative_path(relative)?)
    }
}

async fn check_download(
    dir: &Path,
    archive_mount_path: &ArchivePath,
    clients: &[ClientData],
    version: Option<DateTime<Utc>>,
    source_dir: &Path,
    rng: &mut impl Rng,
) -> Result<()> {
    let local_path = choose_path(source_dir, true, true, true, false, rng)?.unwrap();
    if is_leftover_dir_with_ignored_files(&local_path)? {
        return Ok(());
    }
    let archive_path = archive_subpath(archive_mount_path, source_dir, &local_path)?;
    info!("Checking download: {}, {:?}", archive_path, version);
    let client2 = clients.choose(rng).unwrap();
    let destination = dir.join("tmp_download");
    if destination.exists() {
        remove_dir_all_or_file(&destination)?;
    }
    client2
        .download(
            archive_path,
            destination.to_str().unwrap().parse()?,
            version,
        )
        .await?;
    diff(&local_path, &destination)?;
    Ok(())
}

fn is_ignored(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_str().unwrap();
    name == "target" || name.starts_with("build_")
}

fn remove_dir_all_or_file(path: &Path) -> Result<()> {
    if path.is_dir() {
        remove_dir_all(path)?;
    } else {
        remove_file(path)?;
    }
    Ok(())
}
