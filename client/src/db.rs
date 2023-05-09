use anyhow::{anyhow, Result};
use byteorder::{ByteOrder, LE};
use fs_err as fs;
use rammingen_protocol::{
    ArchivePath, DateTimeUtc, EntryKind, EntryUpdateNumber, EntryVersionData, FileContent,
    RecordTrigger, SourceId,
};
use serde::{Deserialize, Serialize};
use sled::{transaction::ConflictableTransactionError, Transactional};
use std::{fmt::Debug, io, iter, path::Path, str};

use crate::{encryption::decrypt_path, path::SanitizedLocalPath, Ctx};

const KEY_LAST_ENTRY_UPDATE_NUMBER: [u8; 4] = [0, 0, 0, 1];

pub struct Db {
    #[allow(dead_code)]
    db: sled::Db,
    archive_entries: sled::Tree,
    local_entries: sled::Tree,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LocalEntryInfo {
    pub kind: EntryKind,
    pub content: Option<FileContent>,
}

impl LocalEntryInfo {
    pub fn is_same_as_entry(&self, other: &DecryptedEntryVersionData) -> bool {
        if Some(self.kind) != other.kind {
            return false;
        }
        match self.kind {
            EntryKind::File => match (&self.content, &other.content) {
                (Some(content), Some(other)) => {
                    if content.hash != other.hash {
                        return false;
                    }
                    match (content.unix_mode, other.unix_mode) {
                        (None, _) => true,
                        (Some(_), None) => true,
                        (Some(unix_mode), Some(other)) => unix_mode == other,
                    }
                }
                _ => false,
            },
            EntryKind::Directory => true,
        }
    }

    pub fn matches_real(&self, path: impl AsRef<Path>) -> Result<bool> {
        let metadata = fs::metadata(path)?;
        if metadata.is_symlink() {
            return Ok(false);
        }
        if metadata.is_dir() != (self.kind == EntryKind::Directory) {
            return Ok(false);
        }
        if self.kind == EntryKind::File {
            let content = self
                .content
                .as_ref()
                .ok_or_else(|| anyhow!("missing content for file"))?;
            if DateTimeUtc::from(metadata.modified()?) != content.modified_at {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DecryptedEntryVersionData {
    pub path: ArchivePath,
    pub recorded_at: DateTimeUtc,
    pub source_id: SourceId,
    pub record_trigger: RecordTrigger,
    pub kind: Option<EntryKind>,
    pub content: Option<FileContent>,
}

impl DecryptedEntryVersionData {
    pub fn new(ctx: &Ctx, data: EntryVersionData) -> Result<Self> {
        Ok(Self {
            path: decrypt_path(&data.path, &ctx.cipher)?,
            recorded_at: data.recorded_at,
            source_id: data.source_id,
            record_trigger: data.record_trigger,
            kind: data.kind,
            content: data.content,
        })
    }
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let db = sled::open(path)?;
        Ok(Self {
            archive_entries: db.open_tree("archive_entries")?,
            local_entries: db.open_tree("local_entries")?,
            db,
        })
    }

    pub fn get_all_archive_entries(
        &self,
    ) -> impl Iterator<Item = Result<DecryptedEntryVersionData>> + DoubleEndedIterator {
        self.archive_entries
            .iter()
            .map(|pair| Ok(bincode::deserialize::<DecryptedEntryVersionData>(&pair?.1)?))
    }

    pub fn get_archive_entry(
        &self,
        path: &ArchivePath,
    ) -> Result<Option<DecryptedEntryVersionData>> {
        if let Some(value) = self.archive_entries.get(path.0.as_bytes())? {
            Ok(Some(bincode::deserialize::<DecryptedEntryVersionData>(
                &value,
            )?))
        } else {
            Ok(None)
        }
    }

    pub fn get_archive_entries(
        &self,
        path: &ArchivePath,
    ) -> impl Iterator<Item = Result<DecryptedEntryVersionData>> + DoubleEndedIterator {
        let root_entry = (|| {
            let value = self
                .archive_entries
                .get(path.0.as_bytes())?
                .ok_or_else(|| anyhow!("no such archive path: {}", path))?;
            anyhow::Ok(bincode::deserialize::<DecryptedEntryVersionData>(&value)?)
        })();
        let children = if root_entry
            .as_ref()
            .map_or(false, |entry| entry.kind == Some(EntryKind::Directory))
        {
            let mut prefix = path.0.clone();
            prefix.push('/');
            Some(
                self.archive_entries
                    .scan_prefix(prefix)
                    .map(|pair| Ok(bincode::deserialize::<DecryptedEntryVersionData>(&pair?.1)?)),
            )
        } else {
            None
        };
        iter::once(root_entry).chain(children.into_iter().flatten())
    }

    pub fn last_entry_update_number(&self) -> Result<EntryUpdateNumber> {
        Ok(self
            .db
            .get(KEY_LAST_ENTRY_UPDATE_NUMBER)?
            .map(|value| EntryUpdateNumber(LE::read_i64(&value)))
            .unwrap_or(EntryUpdateNumber(0)))
    }

    pub fn update_archive_entries(
        &self,
        updates: &[DecryptedEntryVersionData],
        update_number: EntryUpdateNumber,
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }
        (&*self.db, &self.archive_entries).transaction(|(db, archive_entries)| {
            for update in updates {
                archive_entries.insert(
                    update.path.0.as_bytes(),
                    bincode::serialize(update).map_err(into_abort_err)?,
                )?;
            }
            db.insert(
                &KEY_LAST_ENTRY_UPDATE_NUMBER,
                &update_number.0.to_le_bytes(),
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn get_all_local_entries(
        &self,
    ) -> impl Iterator<Item = Result<(SanitizedLocalPath, LocalEntryInfo)>> + DoubleEndedIterator
    {
        self.local_entries.iter().map(|pair| {
            let (key, value) = pair?;
            let path = SanitizedLocalPath::new(str::from_utf8(&key)?)?;
            let data = bincode::deserialize::<LocalEntryInfo>(&value)?;
            Ok((path, data))
        })
    }

    pub fn get_local_entry(&self, path: &SanitizedLocalPath) -> Result<Option<LocalEntryInfo>> {
        if let Some(value) = self.local_entries.get(path)? {
            Ok(Some(bincode::deserialize::<LocalEntryInfo>(&value)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_local_entry(&self, path: &SanitizedLocalPath, data: &LocalEntryInfo) -> Result<()> {
        self.local_entries.insert(path, bincode::serialize(data)?)?;
        Ok(())
    }

    pub fn remove_local_entry(&self, path: &SanitizedLocalPath) -> Result<()> {
        self.local_entries.remove(path)?;
        Ok(())
    }
}

fn into_abort_err(e: impl Debug) -> ConflictableTransactionError<io::Error> {
    ConflictableTransactionError::Abort(io::Error::new(io::ErrorKind::Other, format!("{e:?}")))
}
