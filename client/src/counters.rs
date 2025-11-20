use {
    crate::{info::pretty_size, Ctx},
    itertools::Itertools,
    serde::{Deserialize, Serialize},
    std::{
        ops::AddAssign,
        sync::atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FinalCounters {
    pub deleted_entries: AtomicU64,
    pub downloaded_entries: AtomicU64,
    pub downloaded_bytes: AtomicU64,
    pub uploaded_entries: AtomicU64,
    pub uploaded_large_files: AtomicU64,
    pub uploaded_bytes: AtomicU64,
}

#[derive(Debug, Default)]
pub struct IntermediateCounters {
    pub queued_download_entries: AtomicU64,
    pub queued_upload_entries: AtomicU64,
    pub unqueued_upload_entries: AtomicU64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationCounters {
    pub deleted_entries: u64,
    pub downloaded_entries: u64,
    pub downloaded_bytes: u64,
    pub uploaded_entries: u64,
    pub uploaded_large_files: u64,
    pub uploaded_bytes: u64,
    pub completed_syncs: u64,
}

impl AddAssign for NotificationCounters {
    fn add_assign(&mut self, rhs: Self) {
        self.deleted_entries += rhs.deleted_entries;
        self.downloaded_entries += rhs.downloaded_entries;
        self.downloaded_bytes += rhs.downloaded_bytes;
        self.uploaded_entries += rhs.uploaded_entries;
        self.uploaded_large_files += rhs.uploaded_large_files;
        self.uploaded_bytes += rhs.uploaded_bytes;
        self.completed_syncs += rhs.completed_syncs;
    }
}

impl NotificationCounters {
    pub fn report(&self, dry_run: bool, accumulated: bool, ctx: &Ctx) -> String {
        let mut output = Vec::new();
        if accumulated {
            output.push(format!(
                "Total sync runs completed: {}",
                self.completed_syncs
            ));
        }
        if self.uploaded_large_files > 0 {
            let size = pretty_size(ctx.config.warn_about_files_larger_than.as_u64());
            if dry_run {
                output.push(format!(
                    "WARN: Would upload {} files larger than {}",
                    self.uploaded_large_files, size,
                ));
            } else {
                output.push(format!(
                    "WARN: Uploaded {} files larger than {}",
                    self.uploaded_large_files, size,
                ));
            }
        }

        if self.downloaded_entries > 0 || self.downloaded_bytes > 0 {
            if dry_run {
                output.push(format!(
                    "Would download {} entries ({})",
                    self.downloaded_entries,
                    pretty_size(self.downloaded_bytes)
                ));
            } else {
                output.push(format!(
                    "Downloaded {} entries ({})",
                    self.downloaded_entries,
                    pretty_size(self.downloaded_bytes)
                ));
            }
        }

        if self.deleted_entries > 0 {
            if dry_run {
                output.push(format!("Would delete {} entries", self.deleted_entries));
            } else {
                output.push(format!("Deleted {} entries", self.deleted_entries));
            }
        }

        let uploaded_entries = self.uploaded_entries;
        let uploaded_bytes = self.uploaded_bytes;
        if uploaded_entries > 0 || uploaded_bytes > 0 {
            if dry_run {
                output.push(format!(
                    "Would upload {} entries ({})",
                    uploaded_entries,
                    pretty_size(uploaded_bytes)
                ));
            } else {
                output.push(format!(
                    "Uploaded {} entries ({})",
                    uploaded_entries,
                    pretty_size(uploaded_bytes)
                ));
            }
        }
        output.into_iter().join("\n")
    }
}

impl From<&FinalCounters> for NotificationCounters {
    fn from(counters: &FinalCounters) -> Self {
        Self {
            deleted_entries: counters.deleted_entries.load(Ordering::Relaxed),
            downloaded_entries: counters.downloaded_entries.load(Ordering::Relaxed),
            downloaded_bytes: counters.downloaded_bytes.load(Ordering::Relaxed),
            uploaded_entries: counters.uploaded_entries.load(Ordering::Relaxed),
            uploaded_large_files: counters.uploaded_large_files.load(Ordering::Relaxed),
            uploaded_bytes: counters.uploaded_bytes.load(Ordering::Relaxed),
            completed_syncs: 0,
        }
    }
}
