use anyhow::{anyhow, Result};
use byteorder::{ByteOrder, LE};
use rammingen_protocol::{ArchivePath, EntryKind, EntryUpdateNumber};
use sled::{transaction::ConflictableTransactionError, Transactional};
use std::{fmt::Debug, io, iter, path::Path, str};

use rammingen_sdk::content::{DecryptedEntryVersionData, LocalEntryInfo};

use crate::path::SanitizedLocalPath;

const KEY_LAST_ENTRY_UPDATE_NUMBER: [u8; 4] = [0, 0, 0, 1];

pub struct Db {
    #[allow(dead_code)]
    db: sled::Db,
    archive_entries: sled::Tree,
    local_entries: sled::Tree,
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
    ) -> impl DoubleEndedIterator<Item = Result<DecryptedEntryVersionData>> {
        self.archive_entries
            .iter()
            .map(|pair| Ok(bincode::deserialize::<DecryptedEntryVersionData>(&pair?.1)?))
    }

    pub fn get_archive_entry(
        &self,
        path: &ArchivePath,
    ) -> Result<Option<DecryptedEntryVersionData>> {
        if let Some(value) = self
            .archive_entries
            .get(path.to_str_without_prefix().as_bytes())?
        {
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
    ) -> impl DoubleEndedIterator<Item = Result<DecryptedEntryVersionData>> {
        let root_entry = (|| {
            let value = self
                .archive_entries
                .get(path.to_str_without_prefix().as_bytes())?
                .ok_or_else(|| anyhow!("no such archive path: {}", path))?;
            anyhow::Ok(bincode::deserialize::<DecryptedEntryVersionData>(&value)?)
        })();
        let children = if root_entry
            .as_ref()
            .map_or(false, |entry| entry.kind == Some(EntryKind::Directory))
        {
            let mut prefix = path.to_str_without_prefix().to_owned();
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
            .map(|value| LE::read_i64(&value))
            .unwrap_or(0)
            .into())
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
                    update.path.to_str_without_prefix().as_bytes(),
                    bincode::serialize(update).map_err(into_abort_err)?,
                )?;
            }
            db.insert(
                &KEY_LAST_ENTRY_UPDATE_NUMBER,
                &i64::from(update_number).to_le_bytes(),
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn get_all_local_entries(
        &self,
    ) -> impl DoubleEndedIterator<Item = Result<(SanitizedLocalPath, LocalEntryInfo)>> {
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
