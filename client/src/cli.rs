use std::{path::PathBuf, str::FromStr};

use anyhow::{anyhow, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use clap::{Parser, Subcommand};
use derive_more::{From, Into};
use rammingen_protocol::{ArchivePath, DateTimeUtc};

use crate::{info::DATE_TIME_FORMAT, path::SanitizedLocalPath};

// #[clap(author, version, about, long_about = None)]
// #[clap(propagate_version = true)]

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(long)]
    pub config: Option<PathBuf>,
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    Sync,
    DryRun,
    Upload {
        local_path: SanitizedLocalPath,
        archive_path: ArchivePath,
    },
    Download {
        archive_path: ArchivePath,
        local_path: SanitizedLocalPath,
        version: Option<DateTimeArg>,
    },
    LocalStatus {
        path: SanitizedLocalPath,
    },
    Ls {
        path: ArchivePath,
        #[arg(short, long)]
        deleted: bool,
    },
    Versions {
        path: ArchivePath,
        #[arg(short, long)]
        recursive: bool,
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
        old_path: ArchivePath,
        new_path: ArchivePath,
    },
    Remove {
        archive_path: ArchivePath,
    },
    Status,
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
