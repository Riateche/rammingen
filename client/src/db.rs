use anyhow::{anyhow, Result};
use byteorder::{ByteOrder, LE};
use rammingen_protocol::{
    ArchivePath, Entry, EntryKind, EntryUpdateNumber, EntryVersionData, FileContent,
};
use serde::{Deserialize, Serialize};
use sled::{transaction::ConflictableTransactionError, Transactional};
use std::{fmt::Debug, io, iter};

use crate::upload::SanitizedLocalPath;

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

impl Db {
    pub fn open() -> Result<Db> {
        let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("cannot find config dir"))?;
        let db = sled::open(config_dir.join("rammingen.db"))?;
        Ok(Self {
            archive_entries: db.open_tree("archive_entries")?,
            local_entries: db.open_tree("local_entries")?,
            db,
        })
    }

    pub fn get_archive_entries(
        &self,
        path: &ArchivePath,
    ) -> impl Iterator<Item = Result<EntryVersionData>> {
        let root_entry = (|| {
            let value = self
                .archive_entries
                .get(path.0.as_bytes())?
                .ok_or_else(|| anyhow!("no such archive path: {}", path))?;
            anyhow::Ok(bincode::deserialize::<EntryVersionData>(&value)?)
        })();
        let children = if root_entry
            .as_ref()
            .map_or(false, |entry| entry.kind == EntryKind::Directory)
        {
            let mut prefix = path.0.clone();
            prefix.push('/');
            Some(
                self.archive_entries
                    .scan_prefix(prefix)
                    .map(|pair| Ok(bincode::deserialize::<EntryVersionData>(&pair?.1)?)),
            )
        } else {
            None
        };
        iter::once(root_entry).chain(children.into_iter().flatten())
    }

    pub fn last_entry_update_number(&self) -> Result<Option<EntryUpdateNumber>> {
        Ok(self
            .db
            .get(KEY_LAST_ENTRY_UPDATE_NUMBER)?
            .map(|value| EntryUpdateNumber(LE::read_i64(&value))))
    }

    pub fn update_archive_entries(&self, updates: &[Entry]) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }
        (&*self.db, &self.archive_entries).transaction(|(db, archive_entries)| {
            for update in updates {
                archive_entries.insert(
                    update.data.path.0.as_bytes(),
                    bincode::serialize(&update.data).map_err(into_abort_err)?,
                )?;
            }
            db.insert(
                &KEY_LAST_ENTRY_UPDATE_NUMBER,
                &updates.last().unwrap().update_number.0.to_le_bytes(),
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn get_local_entries(
        &self,
    ) -> impl Iterator<Item = Result<(SanitizedLocalPath, LocalEntryInfo)>> {
        self.local_entries.iter().rev().map(|pair| {
            let (key, value) = pair?;
            let path = SanitizedLocalPath(String::from_utf8(key.to_vec())?);
            let data = bincode::deserialize::<LocalEntryInfo>(&value)?;
            Ok((path, data))
        })
    }

    pub fn get_local_entry(&self, path: &SanitizedLocalPath) -> Result<Option<LocalEntryInfo>> {
        if let Some(value) = self.local_entries.get(path.0.as_bytes())? {
            Ok(Some(bincode::deserialize::<LocalEntryInfo>(&value)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_local_entry(&self, path: &SanitizedLocalPath, data: &LocalEntryInfo) -> Result<()> {
        self.local_entries
            .insert(path.0.as_bytes(), bincode::serialize(data)?)?;
        Ok(())
    }

    pub fn remove_local_entry(&self, path: &SanitizedLocalPath) -> Result<()> {
        self.local_entries.remove(path.0.as_bytes())?;
        Ok(())
    }
}

fn into_abort_err(e: impl Debug) -> ConflictableTransactionError<io::Error> {
    ConflictableTransactionError::Abort(io::Error::new(io::ErrorKind::Other, format!("{e:?}")))
}
