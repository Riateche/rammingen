use std::path::PathBuf;

use byte_unit::Byte;
use serde::{Deserialize, Serialize};
use url::Url;

use rammingen_protocol::{
    credentials::{AccessToken, EncryptionKey},
    serde_path_with_prefix, ArchivePath,
};

use crate::path::SanitizedLocalPath;
use crate::rules::Rule;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountPoint {
    pub local_path: SanitizedLocalPath,
    #[serde(with = "serde_path_with_prefix")]
    pub archive_path: ArchivePath,
    #[serde(default)]
    pub exclude: Vec<Rule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub always_exclude: Vec<Rule>,
    pub mount_points: Vec<MountPoint>,
    pub encryption_key: EncryptionKey,
    pub server_url: Url,
    pub access_token: AccessToken,
    #[serde(default)]
    pub local_db_path: Option<PathBuf>,
    #[serde(default)]
    pub log_file: Option<PathBuf>,
    #[serde(default = "default_log_filter")]
    pub log_filter: String,

    #[serde(default = "default_warn_about_files_larger_than")]
    pub warn_about_files_larger_than: Byte,
}

fn default_log_filter() -> String {
    "info".into()
}

fn default_warn_about_files_larger_than() -> Byte {
    "50 MB".parse().unwrap()
}
