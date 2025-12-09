use {
    crate::{counters::NotificationCounters, path::SanitizedLocalPath},
    anyhow::{Context as _, Result, bail},
    byteorder::{ByteOrder, LE},
    rammingen_protocol::{
        ArchivePath, ContentHash, DateTimeUtc, EntryKind, EntryUpdateNumber, RecordTrigger,
        SourceId, encoding,
    },
    rammingen_sdk::content::{LocalArchiveEntry, LocalEntry, LocalFileEntry},
    serde::{Deserialize, Serialize},
    sled::{IVec, Transactional, transaction::ConflictableTransactionError},
    std::{fmt::Debug, io, iter, path::Path, str, thread::sleep, time::Duration},
    tracing::{info, warn},
};

const KEY_LAST_ENTRY_UPDATE_NUMBER: [u8; 4] = [0, 0, 0, 1];
const KEY_NOTIFICATION_STATS: [u8; 4] = [0, 0, 0, 2];
const KEY_SERVER_ID: [u8; 4] = [0, 0, 0, 3];

pub struct Db {
    db: sled::Db,
    archive_entries: sled::Tree,
    local_entries: sled::Tree,
}

mod local_entry_version {
    pub const V2: u32 = 2;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LocalEntryLegacyV1 {
    pub kind: EntryKind,
    pub file_data: Option<LocalFileEntryLegacyV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LocalFileEntryLegacyV1 {
    pub modified_at: DateTimeUtc,
    pub original_size: u64,
    pub encrypted_size: u64,
    pub hash: ContentHash,
    pub unix_mode: Option<u32>,
}

mod archive_entry_version {
    pub const V2: u32 = 40002;
}

#[derive(Debug, Serialize, Deserialize)]
struct LocalArchiveEntryLegacyV1 {
    path: ArchivePath,
    recorded_at: DateTimeUtc,
    source_id: SourceId,
    record_trigger: RecordTrigger,
    kind: Option<EntryKind>,
    file_data: Option<LocalFileEntryLegacyV1>,
}

fn decode_local_entry(bytes: &[u8]) -> anyhow::Result<LocalEntry> {
    let version = u32::from_le_bytes(bytes.get(0..4).context("not enough data")?.try_into()?);
    // V1 entry was serialized without version. First 4 bytes were `EntryKind` (0 or 1 in LE).
    if version < local_entry_version::V2 {
        let value = encoding::deserialize::<LocalEntryLegacyV1>(bytes)?;
        Ok(LocalEntry {
            kind: value.kind,
            file_data: value.file_data.map(|value| LocalFileEntry {
                modified_at: value.modified_at,
                original_size: value.original_size,
                encrypted_size: value.encrypted_size,
                hash: value.hash,
                unix_mode: value.unix_mode,
                is_symlink: Some(false),
            }),
        })
    } else if version == local_entry_version::V2 {
        let payload = bytes.get(4..).context("not enough data")?;
        Ok(encoding::deserialize::<LocalEntry>(payload)?)
    } else {
        bail!("unknown encoding version: {version:?}");
    }
}

fn encode_local_entry(entry: &LocalEntry) -> anyhow::Result<Vec<u8>> {
    let mut vec = local_entry_version::V2.to_le_bytes().to_vec();
    encoding::serialize_into(&mut vec, entry)?;
    Ok(vec)
}

fn decode_archive_entry(bytes: &[u8]) -> anyhow::Result<LocalArchiveEntry> {
    let version = u32::from_le_bytes(bytes.get(0..4).context("not enough data")?.try_into()?);
    // V1 entry was serialized without version. First 8 bytes were the length of `path`.
    // Typical max path length is up to 4096, so we assume that it's always less than `V2`.
    if version < archive_entry_version::V2 {
        let value = encoding::deserialize::<LocalArchiveEntryLegacyV1>(bytes)?;
        Ok(LocalArchiveEntry {
            path: value.path,
            recorded_at: value.recorded_at,
            source_id: value.source_id,
            record_trigger: value.record_trigger,
            kind: value.kind,
            file_data: value.file_data.map(|value| LocalFileEntry {
                modified_at: value.modified_at,
                original_size: value.original_size,
                encrypted_size: value.encrypted_size,
                hash: value.hash,
                unix_mode: value.unix_mode,
                is_symlink: Some(false),
            }),
        })
    } else if version == archive_entry_version::V2 {
        let payload = bytes.get(4..).context("not enough data")?;
        Ok(encoding::deserialize::<LocalArchiveEntry>(payload)?)
    } else {
        bail!("unknown encoding version: {version:?}");
    }
}

fn encode_archive_entry(entry: &LocalArchiveEntry) -> anyhow::Result<Vec<u8>> {
    let mut vec = archive_entry_version::V2.to_le_bytes().to_vec();
    encoding::serialize_into(&mut vec, entry)?;
    Ok(vec)
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
    ) -> impl DoubleEndedIterator<Item = Result<LocalArchiveEntry>> {
        self.archive_entries
            .iter()
            .map(|pair| decode_archive_entry(&pair?.1))
    }

    pub fn get_archive_entry(&self, path: &ArchivePath) -> Result<Option<LocalArchiveEntry>> {
        if let Some(value) = self
            .archive_entries
            .get(path.to_str_without_prefix().as_bytes())?
        {
            Ok(Some(decode_archive_entry(&value)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_archive_entries(
        &self,
        path: &ArchivePath,
    ) -> impl DoubleEndedIterator<Item = Result<LocalArchiveEntry>> {
        let root_entry = (|| {
            let value = self
                .archive_entries
                .get(path.to_str_without_prefix().as_bytes())?
                .with_context(|| format!("no such archive path: {}", path))?;
            anyhow::Ok(decode_archive_entry(&value)?)
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
                    .map(|pair| decode_archive_entry(&pair?.1)),
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
        updates: &[LocalArchiveEntry],
        update_number: EntryUpdateNumber,
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }
        (&*self.db, &self.archive_entries).transaction(|(db, archive_entries)| {
            for update in updates {
                archive_entries.insert(
                    update.path.to_str_without_prefix().as_bytes(),
                    encode_archive_entry(update).map_err(into_abort_err)?,
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

    pub fn server_id(&self) -> Result<Option<String>> {
        if let Some(value) = self.db.get(KEY_SERVER_ID)? {
            Ok(Some(String::from_utf8(value.to_vec())?))
        } else {
            Ok(None)
        }
    }

    pub fn set_server_id(&self, value: &str) -> Result<()> {
        self.db.insert(KEY_SERVER_ID, value.as_bytes())?;
        Ok(())
    }

    pub fn get_all_local_entries(&self) -> anyhow::Result<Vec<(SanitizedLocalPath, LocalEntry)>> {
        let load = |key: &IVec, value: &IVec| {
            let path = str::from_utf8(key)?;
            let path = SanitizedLocalPath::new(path)
                .with_context(|| format!("local entry {path:?} is unsupported"))?;
            let data = decode_local_entry(value)
                .with_context(|| format!("invalid data for local entry {path:?}"))?;
            anyhow::Ok((path, data))
        };
        let mut output = Vec::new();
        let mut keys_to_remove = Vec::new();
        for entry in &self.local_entries {
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
            Ok(Some(decode_local_entry(&value)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_local_entry(&self, path: &SanitizedLocalPath, data: &LocalEntry) -> Result<()> {
        self.local_entries.insert(path, encode_local_entry(data)?)?;
        Ok(())
    }

    pub fn remove_local_entry(&self, path: &SanitizedLocalPath) -> Result<()> {
        self.local_entries.remove(path)?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.archive_entries.clear()?;
        self.local_entries.clear()?;
        self.db.clear()?;
        Ok(())
    }
}

fn into_abort_err(e: impl Debug) -> ConflictableTransactionError<io::Error> {
    ConflictableTransactionError::Abort(io::Error::other(format!("{e:?}")))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationStats {
    pub last_notified_at: Option<DateTimeUtc>,
    pub last_successful_sync_at: Option<DateTimeUtc>,
    pub pending_counters: NotificationCounters,
}
