use std::path::Path;

use anyhow::{bail, Result};
use fs_err::{read_dir, symlink_metadata};
use rammingen::unix_mode;

pub fn diff(path1: &Path, path2: &Path) -> Result<()> {
    let meta1 = symlink_metadata(path1)?;
    let meta2 = symlink_metadata(path2)?;
    if meta1.is_symlink() != meta2.is_symlink() {
        bail!(
            "is_symlink mismatch for {} ({}) <-> {} ({})",
            path1.display(),
            meta1.is_symlink(),
            path2.display(),
            meta2.is_symlink(),
        );
    }
    if meta1.is_dir() != meta2.is_dir() {
        bail!(
            "is_dir mismatch for {} ({}) <-> {} ({})",
            path1.display(),
            meta1.is_dir(),
            path2.display(),
            meta2.is_dir(),
        );
    }
    if meta1.is_dir() {
        let mut names1 = Vec::new();
        for entry in read_dir(path1)? {
            let entry = entry?;
            names1.push(entry.file_name());
        }

        let mut names2 = Vec::new();
        for entry in read_dir(path2)? {
            let entry = entry?;
            names2.push(entry.file_name());
        }

        for name2 in &names2 {
            if !names1.contains(name2) {
                bail!("missing {}", path1.join(name2).display());
            }
        }
        for name1 in &names1 {
            if !names2.contains(name1) {
                bail!("missing {}", path2.join(name1).display());
            }
            diff(&path1.join(name1), &path2.join(name1))?;
        }
    } else {
        let content1 = fs_err::read_to_string(path1)?;
        let content2 = fs_err::read_to_string(path2)?;
        if content1 != content2 {
            bail!(
                "content mismatch for {} <-> {}",
                path1.display(),
                path2.display()
            );
        }
        if unix_mode(&meta1) != unix_mode(&meta2) {
            bail!(
                "unix_mode mismatch for {} ({:?}) <-> {} ({:?})",
                path1.display(),
                unix_mode(&meta1),
                path2.display(),
                unix_mode(&meta2),
            );
        }
    }
    Ok(())
}
