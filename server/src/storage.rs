use {
    anyhow::{bail, Context as _, Result},
    fs2::available_space,
    fs_err::{create_dir_all, read_dir, remove_file, rename, symlink_metadata, File, PathExt},
    rammingen_protocol::EncryptedContentHash,
    std::{
        collections::HashMap,
        io::Write,
        path::{Path, PathBuf},
    },
    tempfile::NamedTempFile,
};

/// Manager of the file storage.
///
/// File storage contains a collection of content files identified by `EncryptedContentHash`.
/// These files are arranged in subdirectories to improve performance. The server treats
/// both `EncryptedContentHash` and the content itself as opaque data and doesn't make any assumptions
/// about them (except that `hash.to_url_safe()` must be more than 3 characters long).
#[derive(Debug)]
pub struct Storage {
    root: PathBuf,
    tmp: PathBuf,
}

struct StoragePaths {
    dir_path: PathBuf,
    file_path: PathBuf,
}

/// Returns path to the directory and file where content for `hash` should be stored.
fn storage_paths(root: &Path, hash: &EncryptedContentHash) -> anyhow::Result<StoragePaths> {
    let hash_str = hash.to_url_safe();
    let dir_path = root
        .join(hash_str.get(0..1).context("content hash is too short")?)
        .join(hash_str.get(1..2).context("content hash is too short")?)
        .join(hash_str.get(2..3).context("content hash is too short")?);
    let file_path = dir_path.join(hash_str);
    Ok(StoragePaths {
        dir_path,
        file_path,
    })
}

impl Storage {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.fs_err_try_exists()? {
            bail!("storage root doesn't exist");
        }

        let tmp = root.join("tmp");
        create_dir_all(&tmp)?;

        Ok(Self { root, tmp })
    }

    pub fn create_file(&self) -> Result<NamedTempFile> {
        Ok(NamedTempFile::new_in(&self.tmp)?)
    }

    pub fn commit_file(&self, file: NamedTempFile, hash: &EncryptedContentHash) -> Result<()> {
        file.as_file().flush()?;
        file.as_file().sync_all()?;
        let paths = storage_paths(&self.root, hash)?;
        create_dir_all(&paths.dir_path)?;
        let (_, old_path) = file.keep()?;
        if let Err(err) = rename(&old_path, &paths.file_path) {
            let _ = remove_file(&old_path);
            return Err(err.into());
        }
        Ok(())
    }

    pub fn open_file(&self, hash: &EncryptedContentHash) -> Result<File> {
        let path = storage_paths(&self.root, hash)?.file_path;
        Ok(File::open(path)?)
    }

    pub fn remove_file(&self, hash: &EncryptedContentHash) -> Result<()> {
        let path = storage_paths(&self.root, hash)?.file_path;
        Ok(remove_file(path)?)
    }

    pub fn exists(&self, hash: &EncryptedContentHash) -> Result<bool> {
        let path = storage_paths(&self.root, hash)?.file_path;
        Ok(path.fs_err_try_exists()?)
    }

    pub fn file_size(&self, hash: &EncryptedContentHash) -> Result<u64> {
        let path = storage_paths(&self.root, hash)?.file_path;
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
                    .with_context(|| format!("found path without file name: {:?}", path))?
                    .to_str()
                    .with_context(|| format!("invalid file name: {:?}", path))?;
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
    use {std::io::Read, tempfile::TempDir};

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
