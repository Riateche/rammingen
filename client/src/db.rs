use anyhow::{anyhow, Result};
use rammingen_protocol::{ArchivePath, EntryVersionData};

pub struct Db {
    #[allow(dead_code)]
    db: sled::Db,
    archive_entries: sled::Tree,
}

impl Db {
    pub fn new() -> Result<Db> {
        let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("cannot find config dir"))?;
        let db = sled::open(config_dir.join("rammingen.db"))?;
        let archive_entries = db.open_tree("archive_entries")?;
        Ok(Self {
            db,
            archive_entries,
        })
    }

    pub fn update_archive_entry(&self, update: &EntryVersionData) -> Result<()> {
        self.archive_entries
            .insert(update.path.0.as_bytes(), bincode::serialize(update)?)?;
        Ok(())
    }

    pub fn get_archive_entries(
        &self,
        dir: &ArchivePath,
    ) -> impl Iterator<Item = Result<EntryVersionData>> {
        let mut prefix = dir.0.clone();
        prefix.push('/');
        self.archive_entries
            .scan_prefix(prefix)
            .map(|pair| Ok(bincode::deserialize::<EntryVersionData>(&pair?.1)?))
    }
}
