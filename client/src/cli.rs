use std::{path::PathBuf, str::FromStr};

use anyhow::{anyhow, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use clap::{Parser, Subcommand};
use derive_more::{From, Into};
use rammingen_protocol::{ArchivePath, DateTimeUtc};

use crate::{info::DATE_TIME_FORMAT, path::SanitizedLocalPath};

#[derive(Debug, Parser)]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "File sync and backup utility")]
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
                .ok_or_else(|| anyhow!("ambiguous time"))?
                .into(),
        ))
    }
}
