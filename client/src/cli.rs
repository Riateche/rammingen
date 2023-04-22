use chrono::{DateTime, FixedOffset};
use clap::{Parser, Subcommand};
use rammingen_protocol::ArchivePath;

use crate::upload::SanitizedLocalPath;

// #[clap(author, version, about, long_about = None)]
// #[clap(propagate_version = true)]

#[derive(Parser)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, PartialEq, Eq)]
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
