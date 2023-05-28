use anyhow::{anyhow, bail, Result};
use fs2::available_space;
use fs_err::{create_dir_all, read_dir, remove_file, rename, symlink_metadata, File};
use rammingen_protocol::{util::try_exists, EncryptedContentHash};
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

#[derive(Debug)]
pub struct Storage {
    root: PathBuf,
    tmp: PathBuf,
}

fn storage_paths(root: &Path, hash: &EncryptedContentHash) -> (PathBuf, PathBuf) {
    let hash_str = hash.to_url_safe();
    let dir = root
        .join(&hash_str[0..1])
        .join(&hash_str[1..2])
        .join(&hash_str[2..3]);
    let file_path = dir.join(hash_str);
    (dir, file_path)
}

impl Storage {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !try_exists(&root)? {
            bail!("storage root doesn't exist");
        }

        let tmp = root.join("tmp");
        create_dir_all(&tmp)?;

        Ok(Self { root, tmp })
    }

    pub fn create_file(&self) -> Result<NamedTempFile> {
        Ok(NamedTempFile::new_in(&self.tmp)?)
    }

    pub fn commit_file(&self, mut file: NamedTempFile, hash: &EncryptedContentHash) -> Result<()> {
        file.flush()?;
        let (dir, new_file_path) = storage_paths(&self.root, hash);
        create_dir_all(dir)?;
        let (_, old_path) = file.keep()?;
        if let Err(err) = rename(&old_path, new_file_path) {
            let _ = remove_file(&old_path);
            return Err(err.into());
        }
        Ok(())
    }

    pub fn open_file(&self, hash: &EncryptedContentHash) -> Result<File> {
        let (_, path) = storage_paths(&self.root, hash);
        Ok(File::open(path)?)
    }

    pub fn remove_file(&self, hash: &EncryptedContentHash) -> Result<()> {
        let (_, path) = storage_paths(&self.root, hash);
        Ok(remove_file(path)?)
    }

    pub fn exists(&self, hash: &EncryptedContentHash) -> Result<bool> {
        let (_, path) = storage_paths(&self.root, hash);
        try_exists(path)
    }

    pub fn file_size(&self, hash: &EncryptedContentHash) -> Result<u64> {
        let (_, path) = storage_paths(&self.root, hash);
        Ok(symlink_metadata(path)?.len())
    }

    pub fn available_space(&self) -> Result<u64> {
        Ok(available_space(&self.root)?)
    }

    pub fn all_hashes_and_sizes(&self) -> Result<HashMap<EncryptedContentHash, u64>> {
        let mut map = HashMap::new();
        self.add_hashes_and_sizes(&self.root, &mut map)?;
        Ok(map)
    }

    fn add_hashes_and_sizes(
        &self,
        dir: &Path,
        out: &mut HashMap<EncryptedContentHash, u64>,
    ) -> Result<()> {
        for entry in read_dir(dir)? {
            let path = entry?.path();
            if path == self.tmp {
                continue;
            }
            let meta = symlink_metadata(&path)?;
            if meta.is_symlink() {
                bail!("unexpected symlink");
            }
            if meta.is_dir() {
                self.add_hashes_and_sizes(&path, out)?;
            } else {
                let name = path
                    .file_name()
                    .ok_or_else(|| anyhow!("found path without file name: {:?}", path))?
                    .to_str()
                    .ok_or_else(|| anyhow!("invalid file name: {:?}", path))?;
                let hash = EncryptedContentHash::from_url_safe(name)?;
                let size = meta.len();
                out.insert(hash, size);
            }
        }
        Ok(())
    }
}

#[test]
fn basic() {
    use std::io::Read;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let storage = Storage::new(dir.path().into()).unwrap();
    let hash = EncryptedContentHash::from_encrypted((0..64).collect());
    let mut file = storage.create_file().unwrap();
    writeln!(file, "ok").unwrap();
    storage.commit_file(file, &hash).unwrap();

    let mut file2 = storage.open_file(&hash).unwrap();
    let mut buf = String::new();
    file2.read_to_string(&mut buf).unwrap();
    assert_eq!(buf, "ok\n");
}
