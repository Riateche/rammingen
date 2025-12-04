use {
    crate::crypto::Cipher,
    anyhow::{Context as _, Result},
    rammingen_protocol::{
        ArchivePath, ContentHash, DateTimeUtc, EntryKind, EntryVersionData, RecordTrigger, SourceId,
    },
    serde::{Deserialize, Serialize},
    std::path::Path,
    tempfile::SpooledTempFile,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalFileEntry {
    pub modified_at: DateTimeUtc,
    pub original_size: u64,
    pub encrypted_size: u64,
    pub hash: ContentHash,
    pub unix_mode: Option<u32>,
    pub is_symlink: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalEntry {
    pub kind: EntryKind,
    pub file_data: Option<LocalFileEntry>,
}

fn is_same_optional<T: PartialEq>(local: Option<T>, other: Option<T>) -> bool {
    match (local, other) {
        // Property is not locally supported, so no need to update the local file regardless of other value.
        (None, _) => true,
        // Other version doesn't have a value, so it cannot be used to update the local file.
        (Some(_), None) => true,
        // We need to update local file if the property value changed.
        (Some(local), Some(other)) => local == other,
    }
}

impl LocalEntry {
    pub fn is_same_as_entry(&self, other: &LocalArchiveEntry) -> bool {
        if Some(self.kind) != other.kind {
            return false;
        }
        match self.kind {
            EntryKind::File => match (&self.file_data, &other.file_data) {
                (Some(content), Some(other)) => {
                    content.hash == other.hash
                        && is_same_optional(content.unix_mode, other.unix_mode)
                        && is_same_optional(content.is_symlink, other.is_symlink)
                }
                _ => false,
            },
            EntryKind::Directory => true,
        }
    }

    pub fn matches_real(&self, path: impl AsRef<Path>) -> Result<bool> {
        let metadata = fs_err::symlink_metadata(path)?;
        if metadata.is_dir() != (self.kind == EntryKind::Directory) {
            return Ok(false);
        }
        if self.kind == EntryKind::File {
            let content = self
                .file_data
                .as_ref()
                .context("missing content for file")?;
            if DateTimeUtc::from(metadata.modified()?) != content.modified_at {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LocalArchiveEntry {
    pub path: ArchivePath,
    pub recorded_at: DateTimeUtc,
    pub source_id: SourceId,
    pub record_trigger: RecordTrigger,
    pub kind: Option<EntryKind>,
    pub file_data: Option<LocalFileEntry>,
}

impl LocalArchiveEntry {
    pub fn decrypt(data: EntryVersionData, cipher: &Cipher) -> Result<Self> {
        Ok(Self {
            path: cipher.decrypt_path(&data.path)?,
            recorded_at: data.recorded_at,
            source_id: data.source_id,
            record_trigger: data.record_trigger,
            kind: data.kind,
            file_data: if let Some(content) = data.content {
                Some(LocalFileEntry {
                    modified_at: content.modified_at,
                    original_size: cipher.decrypt_size(&content.original_size)?,
                    encrypted_size: content.encrypted_size,
                    hash: cipher.decrypt_content_hash(&content.hash)?,
                    unix_mode: content.unix_mode,
                    is_symlink: content.is_symlink,
                })
            } else {
                None
            },
        })
    }
}

pub struct EncryptedFileHead {
    pub file: SpooledTempFile,
    pub hash: ContentHash,
    pub original_size: u64,
    pub encrypted_size: u64,
}

#[test]
fn t1() {
    // let entry = LocalEntry {
    //     kind: EntryKind::File,
    //     file_data: Some(LocalFileEntry {
    //         modified_at: Default::default(),
    //         original_size: 1,
    //         encrypted_size: 2,
    //         hash: ContentHash::new([0; 32]),
    //         unix_mode: Some(1),
    //     }),
    // };

    let entry = LocalArchiveEntry {
        path: "ar:/a/c".parse().unwrap(),
        recorded_at: Default::default(),
        source_id: 1.into(),
        record_trigger: RecordTrigger::Move,
        kind: Some(EntryKind::Directory),
        file_data: None,
    };

    println!(
        "{:?}",
        rammingen_protocol::encoding::serialize(&entry).unwrap()
    );
    const MAGIC: u32 = 40000;
    println!("{:?}", MAGIC.to_le_bytes());
}
