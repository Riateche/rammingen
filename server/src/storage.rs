use anyhow::{bail, Result};
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use fs_err::create_dir_all;
use rammingen_protocol::ContentHash;
use std::path::PathBuf;
use tempfile::NamedTempFile;

pub struct Storage {
    root: PathBuf,
    tmp: PathBuf,
}

impl Storage {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.try_exists()? {
            bail!("storage root doesn't exist");
        }

        let tmp = root.join("tmp");
        create_dir_all(&tmp)?;

        Ok(Self { root, tmp })
    }

    pub fn create_file(&self) -> Result<NamedTempFile> {
        Ok(NamedTempFile::new_in(&self.tmp)?)
    }

    pub fn commit_file(&self, file: NamedTempFile, hash: &ContentHash) -> Result<()> {
        let hash_str = BASE64_URL_SAFE_NO_PAD.encode(&hash.0);
        todo!()
    }
}
