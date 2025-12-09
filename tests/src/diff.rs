use {
    crate::is_ignored,
    anyhow::{Result, bail},
    fs_err::{read_dir, symlink_metadata},
    rammingen::unix_mode,
    std::path::Path,
};

pub fn is_leftover_dir_with_ignored_files(path: &Path) -> Result<bool> {
    let meta = symlink_metadata(path)?;
    if !meta.is_dir() {
        return Ok(false);
    }
    let mut any_ignored = false;
    for entry in read_dir(path)? {
        let entry_path = entry?.path();
        if is_ignored(&entry_path) || is_leftover_dir_with_ignored_files(&entry_path)? {
            any_ignored = true;
        } else {
            return Ok(false);
        }
    }
    Ok(any_ignored)
}

pub fn diff(path1: &Path, path2: &Path) -> Result<()> {
    if is_ignored(path1) {
        assert!(is_ignored(path2));
        return Ok(());
    }
    assert!(!is_ignored(path2));

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
        if is_leftover_dir_with_ignored_files(path1)? || is_leftover_dir_with_ignored_files(path2)?
        {
            return Ok(());
        }
        bail!(
            "is_dir mismatch for {} ({}) <-> {} ({})",
            path1.display(),
            meta1.is_dir(),
            path2.display(),
            meta2.is_dir(),
        );
    }
    if meta1.is_symlink() {
        let target1 = fs_err::read_link(path1)?;
        let target2 = fs_err::read_link(path2)?;
        if target1 != target2 {
            bail!(
                "symlink target mismatch: {} -> {}, {} -> {}",
                path1.display(),
                target1.display(),
                path2.display(),
                target2.display(),
            );
        }
    } else if meta1.is_dir() {
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
            if !names1.contains(name2)
                && !is_ignored(&path1.join(name2))
                && !is_leftover_dir_with_ignored_files(&path2.join(name2))?
            {
                bail!("missing {}", path1.join(name2).display());
            }
        }
        for name1 in &names1 {
            if names2.contains(name1) {
                diff(&path1.join(name1), &path2.join(name1))?;
            } else if !is_ignored(&path2.join(name1))
                && !is_leftover_dir_with_ignored_files(&path1.join(name1))?
            {
                bail!("missing {}", path2.join(name1).display());
            }
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
                "unix_mode mismatch for {} ({:#o}) <-> {} ({:#o})",
                path1.display(),
                unix_mode(&meta1).unwrap(),
                path2.display(),
                unix_mode(&meta2).unwrap(),
            );
        }
    }
    Ok(())
}

pub fn diff_ignored(path1: &Path, path2: &Path) -> Result<()> {
    if is_ignored(path1) {
        assert!(is_ignored(path2));
        diff(path1, path2)?;
        return Ok(());
    }
    assert!(!is_ignored(path2));
    let meta1 = symlink_metadata(path1)?;
    let meta2 = symlink_metadata(path2)?;
    if !meta1.is_dir() || !meta2.is_dir() {
        return Ok(());
    }

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
        if !names1.contains(name2) && is_ignored(&path1.join(name2)) {
            bail!("missing {}", path1.join(name2).display());
        }
    }
    for name1 in &names1 {
        if names2.contains(name1) {
            diff_ignored(&path1.join(name1), &path2.join(name1))?;
        } else if is_ignored(&path2.join(name1)) {
            bail!("missing {}", path2.join(name1).display());
        }
    }
    Ok(())
}
