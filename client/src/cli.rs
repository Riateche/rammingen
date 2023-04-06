use anyhow::anyhow;
use chrono::{DateTime, FixedOffset};
use clap::{Parser, Subcommand};
use core::fmt;
use std::{path::PathBuf, str::FromStr};

// #[clap(author, version, about, long_about = None)]
// #[clap(propagate_version = true)]

#[derive(Parser)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArchivePath(pub String);

impl FromStr for ArchivePath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.strip_prefix("ar:")
            .ok_or_else(|| anyhow!("archive path must start with 'ar:'"))
            .map(|s| Self(s.into()))
    }
}

impl fmt::Display for ArchivePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ar:{}", self.0)
    }
}

#[derive(Subcommand, PartialEq, Eq)]
pub enum Command {
    Sync,
    DryRun,
    Upload {
        local_path: PathBuf,
        archive_path: ArchivePath,
    },
    Download {
        archive_path: ArchivePath,
        local_path: PathBuf,
        version: Option<DateTime<FixedOffset>>,
        // #[clap(short, long)]
        // replace: bool,
    },
    ListDirectory {
        path: String,
    },
    History {
        archive_path: ArchivePath,
        time_spec: String, // TODO
    },
    Reset {
        archive_path: ArchivePath,
        version: DateTime<FixedOffset>,
    },
    Move {
        archive_path: ArchivePath,
        new_archive_path: ArchivePath,
    },
    Remove {
        archive_path: ArchivePath,
    },
    RemoveVersion {
        archive_path: ArchivePath,
        version: DateTime<FixedOffset>,
    },
}
