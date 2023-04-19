use std::sync::atomic::{AtomicU64, Ordering};

use crate::term::info;

#[derive(Debug, Default)]
pub struct Counters {
    pub scanned_entries: AtomicU64,
    pub modified_files: AtomicU64,
    pub sent_to_server: AtomicU64,
    pub updated_on_server: AtomicU64,
}

impl Counters {
    pub fn report(&self) {
        let scanned_entries = self.scanned_entries.load(Ordering::Relaxed);
        let modified_files = self.modified_files.load(Ordering::Relaxed);
        let sent_to_server = self.sent_to_server.load(Ordering::Relaxed);
        let updated_on_server = self.updated_on_server.load(Ordering::Relaxed);
        info(format!("scanned {} entries", scanned_entries));
        if modified_files > 0 {
            info(format!("found {} modified files", modified_files));
        }
        if sent_to_server > 0 {
            info(format!("sent {} entries to server", sent_to_server));
        }
        if updated_on_server > 0 {
            info(format!("updated {} entries on server", updated_on_server));
        }
    }
}
