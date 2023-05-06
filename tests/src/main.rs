mod diff;
mod shuffle;

use std::{
    net::SocketAddr,
    path::{self, Path, PathBuf},
};

use anyhow::{bail, Result};
use chrono::{DateTime, FixedOffset};
use diff::{diff, diff_ignored, is_leftover_dir_with_ignored_files};
use fs_err::{create_dir_all, read_dir, remove_dir_all, remove_file};
use portpicker::pick_unused_port;
use rammingen::{
    cli::{Cli, Command},
    config::{EncryptionKey, MountPoint},
    path::SanitizedLocalPath,
    rules::Rule,
    term::{clear_status, debug, error, info},
};
use rammingen_protocol::ArchivePath;
use rand::{seq::SliceRandom, thread_rng, Rng};
use shuffle::{choose_path, shuffle};
use sqlx::{query, PgPool};
use tempfile::TempDir;

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
    let r = try_main().await;
    clear_status();
    if let Err(err) = r {
        error(format!("{:?}", err));
    }
}

async fn try_main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let database_url = std::env::args().nth(1).expect("missing arg");
    rammingen_server::migrate(&database_url).await?;

    // TODO: remove into_path
    let dir = TempDir::new()?.into_path();
    debug(format!("dir: {}", dir.display()));
    let storage_path = dir.join("storage");
    create_dir_all(&storage_path)?;

    let port = pick_unused_port().expect("failed to pick port");
    let server_config = rammingen_server::Config {
        bind_addr: SocketAddr::new("127.0.0.1".parse()?, port),
        database_url: database_url.clone(),
        storage_path,
    };

    let encryption_key = EncryptionKey::generate();
    let db_pool = PgPool::connect(&database_url).await?;
    let mut clients = Vec::new();
    let archive_mount_path: ArchivePath = "ar:/my_files".parse()?;
    for client_index in 0..3 {
        let client_dir = dir.join(format!("client{client_index}"));
        let mount_dir = client_dir.join("mount1");
        create_dir_all(&mount_dir)?;
        let token = format!("token{client_index}");
        let config = rammingen::config::Config {
            always_exclude: vec![Rule::NameEquals("target".into())],
            mount_points: vec![MountPoint {
                local_path: mount_dir.to_str().unwrap().parse()?,
                archive_path: archive_mount_path.clone(),
                exclude: vec![Rule::NameMatches("^build_".parse()?)],
            }],
            encryption_key: encryption_key.clone(),
            server_url: format!("http://127.0.0.1:{port}/"),
            token: token.clone(),
            salt: "salt1".into(),
            local_db_path: Some(client_dir.join("db")),
        };
        clients.push(ClientData { config, mount_dir });

        query("INSERT INTO sources(name, secret) VALUES ($1, $2)")
            .bind(format!("client{client_index}"))
            .bind(token)
            .execute(&db_pool)
            .await?;
    }

    tokio::spawn(async move {
        if let Err(err) = rammingen_server::run(server_config).await {
            clear_status();
            error(format!("server failed: {err:?}"));
            std::process::exit(1);
        }
    });

    for _ in 0..1000 {
        let index = thread_rng().gen_range(0..clients.len());
        for _ in 0..thread_rng().gen_range(1..=3) {
            debug(format!("shuffling mount for client {index}"));
            shuffle(&clients[index].mount_dir)?;
            debug(format!("syncing client {index}"));
            clients[index].sync().await?;
        }
        for (index2, client) in clients.iter().enumerate() {
            if index2 != index {
                debug(format!("syncing client {index2}"));
                let before_sync_snapshot = dir.join("snapshot");
                copy_dir_all(&client.mount_dir, &before_sync_snapshot)?;
                client.sync().await?;
                diff_ignored(&client.mount_dir, &before_sync_snapshot)?;
                remove_dir_all(&before_sync_snapshot)?;
            }
        }
        for client in &clients[1..] {
            diff(&clients[0].mount_dir, &client.mount_dir)?;
        }
        check_download_latest(&dir, &archive_mount_path, &clients).await?;
    }

    Ok(())
}

struct ClientData {
    mount_dir: PathBuf,
    config: rammingen::config::Config,
}

impl ClientData {
    async fn sync(&self) -> Result<()> {
        rammingen::run(
            Cli {
                config: None,
                command: Command::Sync,
            },
            self.config.clone(),
        )
        .await
    }
    async fn download(
        &self,
        archive_path: ArchivePath,
        local_path: SanitizedLocalPath,
        version: Option<DateTime<FixedOffset>>,
    ) -> Result<()> {
        rammingen::run(
            Cli {
                config: None,
                command: Command::Download {
                    archive_path,
                    local_path,
                    version,
                },
            },
            self.config.clone(),
        )
        .await
    }
}

async fn check_download_latest(
    dir: &Path,
    archive_mount_path: &ArchivePath,
    clients: &[ClientData],
) -> Result<()> {
    let client1 = clients.choose(&mut thread_rng()).unwrap();
    let local_path = choose_path(&client1.mount_dir, true, true, true, false)?.unwrap();
    if is_leftover_dir_with_ignored_files(&local_path)? {
        return Ok(());
    }
    let archive_path = if local_path == client1.mount_dir {
        archive_mount_path.clone()
    } else {
        let relative = local_path.strip_prefix(&client1.mount_dir)?;
        let mut path = archive_mount_path.clone();
        for component in relative.components() {
            if let path::Component::Normal(name) = component {
                path = path.join(name.to_str().unwrap())?;
            } else {
                bail!("invalid path: {:?}", relative);
            }
        }
        path
    };
    info(format!("Checking download: {}", archive_path));
    let client2 = clients.choose(&mut thread_rng()).unwrap();
    let destination = dir.join("tmp_download");
    client2
        .download(archive_path, destination.to_str().unwrap().parse()?, None)
        .await?;
    diff(&local_path, &destination)?;
    if destination.is_dir() {
        remove_dir_all(&destination)?;
    } else {
        remove_file(&destination)?;
    }

    Ok(())
}

fn is_ignored(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_str().unwrap();
    name == "target" || name.starts_with("build_")
}
