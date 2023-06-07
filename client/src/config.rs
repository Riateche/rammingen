use aes_siv::aead::OsRng;
use aes_siv::{Aes256SivAead, KeyInit};
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use core::fmt;
use derivative::Derivative;
use generic_array::GenericArray;
use rammingen_protocol::{serde_path_with_prefix, ArchivePath};
use reqwest::Url;
use serde::de::Error;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use typenum::U64;

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

#[derive(Clone)]
pub struct EncryptionKey(GenericArray<u8, U64>);

impl EncryptionKey {
    pub fn generate() -> Self {
        Self(Aes256SivAead::generate_key(&mut OsRng))
    }

    pub fn get(&self) -> &GenericArray<u8, U64> {
        &self.0
    }
}

impl fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptionKey").finish()
    }
}

impl<'de> Deserialize<'de> for EncryptionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        let binary = BASE64_URL_SAFE_NO_PAD
            .decode(string)
            .map_err(D::Error::custom)?;
        let array = <[u8; 64]>::try_from(binary).map_err(|vec| {
            D::Error::custom(format!(
                "invalid encryption key length, expected 64, got {}",
                vec.len()
            ))
        })?;
        Ok(Self(array.into()))
    }
}

impl Serialize for EncryptionKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        BASE64_URL_SAFE_NO_PAD.encode(self.0).serialize(serializer)
    }
}

#[derive(Derivative, Clone, Serialize, Deserialize)]
#[derivative(Debug)]
pub struct Config {
    pub always_exclude: Vec<Rule>,
    pub mount_points: Vec<MountPoint>,
    pub encryption_key: EncryptionKey,
    pub server_url: Url,
    #[derivative(Debug = "ignore")]
    pub access_token: String,
    #[serde(default)]
    pub local_db_path: Option<PathBuf>,
    #[serde(default)]
    pub log_file: Option<PathBuf>,
    #[serde(default = "default_log_filter")]
    pub log_filter: String,
}

fn default_log_filter() -> String {
    "info".into()
}
