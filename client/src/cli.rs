use {
    crate::{info::DATE_TIME_FORMAT, path::SanitizedLocalPath},
    anyhow::{ Context, Result},
    chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone},
    clap::{Parser, Subcommand},
    derive_more::{From, Into},
    rammingen_protocol::{ArchivePath, DateTimeUtc},
    std::{path::PathBuf, str::FromStr},
};

#[derive(Debug, Parser)]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = about())]
pub struct Cli {
    /// Path to config.
    ///
    /// If omitted, default path is used:
    ///
    /// - $XDG_CONFIG_HOME/rammingen.conf on Linux
    ///
    /// - $HOME/Library/Application Support/rammingen.conf on macOS
    ///
    /// - %APPDATA%\rammingen.conf on Windows
    #[clap(long)]
    pub config: Option<PathBuf>,
    #[clap(subcommand)]
    pub command: Command,
}

fn display_path(path: anyhow::Result<PathBuf>) -> String {
    match path {
        Ok(path) => path.display().to_string(),
        Err(err) => {
            eprintln!("{err}");
            "(error)".into()
        }
    }
}

fn about() -> String {
    format!(
        "File sync and backup utility\nDefault config location: {}\nDefault log location: {}",
        display_path(default_config_path()),
        display_path(default_log_path()),
    )
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Show what will happen on sync.
    DryRun,
    /// Sync all mount point with the server.
    Sync,
    /// Upload a file or directory to the server.
    Upload {
        local_path: SanitizedLocalPath,
        archive_path: ArchivePath,
    },
    /// Download a file or directory from the server.
    Download {
        archive_path: ArchivePath,
        local_path: SanitizedLocalPath,
        /// Timestamp of the version to be downloaded (in local time zone).
        /// If omitted, the latest version is downloaded.
        /// Accepted timestamp format: %Y-%m-%d_%H:%M:%S
        version: Option<DateTimeArg>,
    },
    /// Shows information about a local path.
    LocalStatus { path: SanitizedLocalPath },
    /// Shows information about an archive path.
    Ls {
        path: ArchivePath,
        /// Also shows deleted entries.
        #[arg(short, long)]
        deleted: bool,
    },
    /// Shows the list of available versions for an archive path.
    History {
        path: ArchivePath,
        /// Also shows versions of all nested paths.
        #[arg(short, long)]
        recursive: bool,
    },
    /// Set the specified version as the current version of an archive path.
    Reset {
        archive_path: ArchivePath,
        /// Accepted timestamp format: %Y-%m-%d_%H:%M:%S
        version: DateTime<FixedOffset>,
    },
    /// Move (rename) data from one archive path to another.
    Move {
        old_path: ArchivePath,
        new_path: ArchivePath,
    },
    /// Remove an archive path.
    Remove { archive_path: ArchivePath },
    /// Shows server status.
    Status,
    /// Initiates an integrity check on the server.
    CheckIntegrity,
    /// Generates a new encryption key.
    GenerateEncryptionKey,
}

#[derive(Debug, Clone, PartialEq, Eq, From, Into)]
pub struct DateTimeArg(pub DateTimeUtc);

impl FromStr for DateTimeArg {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self> {
        let naive: NaiveDateTime = NaiveDateTime::parse_from_str(input, DATE_TIME_FORMAT)?;
        Ok(Self(
            Local
                .from_local_datetime(&naive)
                .single()
                .context("ambiguous time")?
                .into(),
        ))
    }
}

pub fn default_config_path() -> anyhow::Result<PathBuf> {
    let config_dir = dirs::config_dir().context("cannot find config dir")?;
    Ok(config_dir.join("rammingen.conf"))
}

pub fn default_log_path() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("cannot find data dir")?
        .join("rammingen.log"))
}
