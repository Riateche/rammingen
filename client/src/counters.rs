use std::sync::atomic::{AtomicU64, Ordering};

use tracing::{info, warn};

use crate::{info::pretty_size, Ctx};

#[derive(Debug, Default)]
pub struct Counters {
    pub deleted_entries: AtomicU64,
    pub downloaded_entries: AtomicU64,
    pub downloaded_bytes: AtomicU64,
    pub uploaded_entries: AtomicU64,
    pub uploaded_large_files: AtomicU64,
    pub uploaded_bytes: AtomicU64,

    pub queued_download_entries: AtomicU64,
    pub queued_upload_entries: AtomicU64,
    pub unqueued_upload_entries: AtomicU64,
}

impl Counters {
    pub fn report(&self, dry_run: bool, ctx: &Ctx) {
        let uploaded_large_files = self.uploaded_large_files.load(Ordering::Relaxed);
        if uploaded_large_files > 0 {
            let size = pretty_size(ctx.config.warn_about_files_larger_than.get_bytes());
            if dry_run {
                warn!(
                    "Would upload {} files larger than {}",
                    uploaded_large_files, size,
                );
            } else {
                warn!(
                    "Uploaded {} files larger than {}",
                    uploaded_large_files, size,
                );
            }
        }

        let downloaded_entries = self.downloaded_entries.load(Ordering::Relaxed);
        let downloaded_bytes = self.downloaded_bytes.load(Ordering::Relaxed);
        if downloaded_entries > 0 || downloaded_bytes > 0 {
            if dry_run {
                info!(
                    "Would download {} entries ({})",
                    downloaded_entries,
                    pretty_size(downloaded_bytes)
                );
            } else {
                info!(
                    "Downloaded {} entries ({})",
                    downloaded_entries,
                    pretty_size(downloaded_bytes)
                );
            }
        }

        let deleted_entries = self.deleted_entries.load(Ordering::Relaxed);
        if deleted_entries > 0 {
            if dry_run {
                info!("Would delete {} entries", deleted_entries);
            } else {
                info!("Deleted {} entries", deleted_entries);
            }
        }

        let uploaded_entries = self.uploaded_entries.load(Ordering::Relaxed);
        let uploaded_bytes = self.uploaded_bytes.load(Ordering::Relaxed);
        if uploaded_entries > 0 || uploaded_bytes > 0 {
            if dry_run {
                info!(
                    "Would upload {} entries ({})",
                    uploaded_entries,
                    pretty_size(uploaded_bytes)
                );
            } else {
                info!(
                    "Uploaded {} entries ({})",
                    uploaded_entries,
                    pretty_size(uploaded_bytes)
                );
            }
        }
    }
}
