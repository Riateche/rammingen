use {
    crate::{counters::NotificationCounters, path::SanitizedLocalPath},
    anyhow::{anyhow, Context as _, Result},
    byteorder::{ByteOrder, LE},
    rammingen_protocol::{ArchivePath, DateTimeUtc, EntryKind, EntryUpdateNumber},
    rammingen_sdk::content::{DecryptedEntryVersion, LocalEntry},
    serde::{Deserialize, Serialize},
    sled::{transaction::ConflictableTransactionError, IVec, Transactional},
    std::{fmt::Debug, io, iter, path::Path, str, thread::sleep, time::Duration},
    tracing::{info, warn},
};

const KEY_LAST_ENTRY_UPDATE_NUMBER: [u8; 4] = [0, 0, 0, 1];
const KEY_NOTIFICATION_STATS: [u8; 4] = [0, 0, 0, 2];

pub struct Db {
    #[allow(dead_code)]
    db: sled::Db,
    archive_entries: sled::Tree,
    local_entries: sled::Tree,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let mut logged_error = false;
        let db = loop {
            match sled::open(path) {
                Ok(db) => break db,
                Err(err) => {
                    if !logged_error {
                        warn!("Failed to open database: {err}");
                        info!("Retrying...");
                        logged_error = true;
                    }
                    sleep(Duration::from_millis(100));
                }
            }
        };
        if logged_error {
            info!("Opened database");
        }
        Ok(Self {
            archive_entries: db.open_tree("archive_entries")?,
            local_entries: db.open_tree("local_entries")?,
            db,
        })
    }

    pub fn get_all_archive_entries(
        &self,
    ) -> impl DoubleEndedIterator<Item = Result<DecryptedEntryVersion>> {
        self.archive_entries
            .iter()
            .map(|pair| Ok(bincode::deserialize::<DecryptedEntryVersion>(&pair?.1)?))
    }

    pub fn get_archive_entry(&self, path: &ArchivePath) -> Result<Option<DecryptedEntryVersion>> {
        if let Some(value) = self
            .archive_entries
            .get(path.to_str_without_prefix().as_bytes())?
        {
            Ok(Some(bincode::deserialize::<DecryptedEntryVersion>(&value)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_archive_entries(
        &self,
        path: &ArchivePath,
    ) -> impl DoubleEndedIterator<Item = Result<DecryptedEntryVersion>> {
        let root_entry = (|| {
            let value = self
                .archive_entries
                .get(path.to_str_without_prefix().as_bytes())?
                .ok_or_else(|| anyhow!("no such archive path: {}", path))?;
            anyhow::Ok(bincode::deserialize::<DecryptedEntryVersion>(&value)?)
        })();
        let children = if root_entry
            .as_ref()
            .is_ok_and(|entry| entry.kind == Some(EntryKind::Directory))
        {
            let mut prefix = path.to_str_without_prefix().to_owned();
            prefix.push('/');
            Some(
                self.archive_entries
                    .scan_prefix(prefix)
                    .map(|pair| Ok(bincode::deserialize::<DecryptedEntryVersion>(&pair?.1)?)),
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
        updates: &[DecryptedEntryVersion],
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

    pub fn notification_stats(&self) -> Result<NotificationStats> {
        if let Some(value) = self.db.get(KEY_NOTIFICATION_STATS)? {
            Ok(serde_json::from_slice(&value)?)
        } else {
            Ok(NotificationStats::default())
        }
    }

    pub fn set_notification_stats(&self, value: &NotificationStats) -> Result<()> {
        self.db
            .insert(KEY_NOTIFICATION_STATS, serde_json::to_vec(value)?)?;
        Ok(())
    }

    pub fn get_all_local_entries(&self) -> anyhow::Result<Vec<(SanitizedLocalPath, LocalEntry)>> {
        let load = |key: &IVec, value: &IVec| {
            let path = str::from_utf8(key)?;
            let path = SanitizedLocalPath::new(path)
                .with_context(|| format!("local entry {path:?} is unsupported"))?;
            let data = bincode::deserialize::<LocalEntry>(value)
                .with_context(|| format!("invalid data for local entry {path:?}"))?;
            anyhow::Ok((path, data))
        };
        let mut output = Vec::new();
        let mut keys_to_remove = Vec::new();
        for entry in self.local_entries.iter() {
            let (key, value) = entry.context("database iterator failed")?;
            match load(&key, &value) {
                Ok((path, data)) => output.push((path, data)),
                Err(err) => {
                    warn!("removing invalid local entry: {err}");
                    keys_to_remove.push(key);
                }
            }
        }
        for key in keys_to_remove {
            self.local_entries.remove(key)?;
        }
        Ok(output)
    }

    pub fn get_local_entry(&self, path: &SanitizedLocalPath) -> Result<Option<LocalEntry>> {
        if let Some(value) = self.local_entries.get(path)? {
            Ok(Some(bincode::deserialize::<LocalEntry>(&value)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_local_entry(&self, path: &SanitizedLocalPath, data: &LocalEntry) -> Result<()> {
        self.local_entries.insert(path, bincode::serialize(data)?)?;
        Ok(())
    }

    pub fn remove_local_entry(&self, path: &SanitizedLocalPath) -> Result<()> {
        self.local_entries.remove(path)?;
        Ok(())
    }
}

fn into_abort_err(e: impl Debug) -> ConflictableTransactionError<io::Error> {
    ConflictableTransactionError::Abort(io::Error::other(format!("{e:?}")))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationStats {
    pub last_notified_at: Option<DateTimeUtc>,
    pub pending_counters: NotificationCounters,
}
