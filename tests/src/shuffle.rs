use {
    crate::{diff::is_leftover_dir_with_ignored_files, is_ignored},
    anyhow::Result,
    fs_err::{create_dir, read_dir, remove_dir_all, remove_file, rename, symlink_metadata, write},
    rammingen::{path::PathExt, symlinks_enabled},
    rand::{
        distr::{weighted::WeightedIndex, Alphanumeric, SampleString},
        prelude::Distribution,
        seq::IndexedRandom,
        Rng,
    },
    std::{
        path::{Path, PathBuf},
        thread::sleep,
        time::Duration,
    },
    tracing::debug,
};

#[allow(clippy::collapsible_if)]
fn find_paths_inner(
    dir: &Path,
    allow_files: bool,
    allow_dirs: bool,
    allow_root: bool,
    allow_ignored: bool,
    allow_symlinks: bool,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    if allow_dirs && allow_root {
        output.push(dir.into());
    }
    for entry in read_dir(dir)? {
        let entry = entry?.path();
        if !allow_ignored && is_ignored(&entry) {
            continue;
        }
        let metadata = symlink_metadata(&entry)?;
        if metadata.is_symlink() {
            if allow_symlinks {
                output.push(entry);
            }
        } else if metadata.is_dir() {
            find_paths_inner(
                &entry,
                allow_files,
                allow_dirs,
                true,
                allow_ignored,
                allow_symlinks,
                output,
            )?;
            if allow_dirs {
                output.push(entry);
            }
        } else if metadata.is_file() {
            if allow_files {
                output.push(entry);
            }
        }
    }

    Ok(())
}

pub fn random_name(allow_ignored: bool, rng: &mut impl Rng) -> String {
    if allow_ignored && rng.random_bool(0.1) {
        // ignored name
        "target".into()
    } else if allow_ignored && rng.random_bool(0.1) {
        // ignored name
        format!("build_{}", rng.random_range(0..1000))
    } else {
        let name_len = rng.random_range(1..=10);
        Alphanumeric.sample_string(rng, name_len)
    }
}

pub fn random_content(rng: &mut impl Rng) -> String {
    let content_len = rng.random_range(0..=3_000_000);
    Alphanumeric.sample_string(rng, content_len)
}

pub fn choose_path(
    dir: &Path,
    allow_files: bool,
    allow_dirs: bool,
    allow_root: bool,
    allow_ignored: bool,
    allow_symlinks: bool,
    rng: &mut impl Rng,
) -> Result<Option<PathBuf>> {
    let mut paths = Vec::new();
    find_paths_inner(
        dir,
        allow_files,
        allow_dirs,
        allow_root,
        allow_ignored,
        allow_symlinks,
        &mut paths,
    )?;
    Ok(paths.choose(rng).cloned())
}

fn create(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let parent = choose_path(dir, false, true, true, true, false, rng)?.unwrap();
    if is_leftover_dir_with_ignored_files(&parent)? {
        return Ok(());
    }
    let path = parent.join(random_name(true, rng));
    if path.try_exists_nofollow()? {
        return Ok(());
    }
    if rng.random_bool(0.1) {
        // dir
        create_dir(&path)?;
        debug!("Created dir {}", path.display());
    } else if rng.random_bool(0.1) && symlinks_enabled() {
        // symlink
        #[cfg(target_family = "unix")]
        {
            use {anyhow::Context as _, pathdiff::diff_paths};

            let target_absolute = choose_path(dir, true, true, true, true, true, rng)?.unwrap();
            if target_absolute == parent {
                return Ok(());
            }
            let target_relative =
                diff_paths(&target_absolute, parent).context("diff_paths failed")?;
            fs_err::os::unix::fs::symlink(&target_relative, &path)?;
        }
        #[cfg(not(target_family = "unix"))]
        {
            unreachable!();
        }
    } else {
        // regular file
        write(&path, random_content(rng))?;
        debug!("Created file {}", path.display());
    }
    Ok(())
}

fn file_to_dir(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let Some(path) = choose_path(dir, true, false, false, true, true, rng)? else {
        return Ok(());
    };
    remove_file(&path)?;
    create_dir(&path)?;
    debug!("Replaced file with dir {}", path.display());
    Ok(())
}

fn dir_to_file(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let Some(path) = choose_path(dir, false, true, false, true, false, rng)? else {
        return Ok(());
    };
    remove_dir_all(&path)?;
    write(&path, random_content(rng))?;
    debug!("Replaced dir with file {}", path.display());
    Ok(())
}

fn random_rename(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let Some(from) = choose_path(dir, true, true, false, true, true, rng)? else {
        return Ok(());
    };
    let to = if rng.random_bool(0.2) {
        choose_path(dir, false, true, true, true, false, rng)?
            .unwrap()
            .join(random_name(true, rng))
    } else {
        from.parent().unwrap().join(random_name(true, rng))
    };
    if !to.try_exists_nofollow()? && !to.starts_with(&from) {
        rename(&from, &to)?;
        debug!("Renamed {} -> {}", from.display(), to.display());
    }
    Ok(())
}

fn edit(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let Some(path) = choose_path(dir, true, false, false, true, false, rng)? else {
        return Ok(());
    };
    if symlink_metadata(&path)?.modified()?.elapsed()? < Duration::from_millis(50) {
        sleep(Duration::from_millis(50));
    }
    write(&path, random_content(rng))?;
    //let new_modified = symlink_metadata(&path)?.modified()?;
    debug!("Edited file {}", path.display());
    Ok(())
}

fn change_mode(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    #[cfg(target_family = "unix")]
    {
        use std::{fs::Permissions, os::unix::prelude::PermissionsExt};

        let Some(path) = choose_path(dir, true, false, false, true, false, rng)? else {
            return Ok(());
        };
        let mode = [0o777, 0o774, 0o744, 0o700, 0o666, 0o664, 0o644, 0o600]
            .choose(rng)
            .unwrap();

        fs_err::set_permissions(&path, Permissions::from_mode(*mode))?;
        debug!("Changed mode of file {} to {:#o}", path.display(), mode);
    }
    #[cfg(not(target_family = "unix"))]
    let _ = (dir, rng);
    Ok(())
}

fn delete(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    if rng.random_bool(0.1) {
        // dir
        let Some(path) = choose_path(dir, false, true, false, true, false, rng)? else {
            return Ok(());
        };
        remove_dir_all(&path)?;
        debug!("Removed dir {}", path.display());
    } else {
        // file
        let Some(path) = choose_path(dir, true, false, false, true, true, rng)? else {
            return Ok(());
        };
        remove_file(&path)?;
        debug!("Removed file {}", path.display());
    }
    Ok(())
}

type Shuffler<R> = fn(dir: &Path, &mut R) -> Result<()>;

pub fn shuffle<R: Rng>(dir: &Path, rng: &mut R) -> Result<()> {
    let num_mutations = rng.random_range(1..=30);
    let shufflers: &[(Shuffler<R>, i32)] = &[
        (create, 10),
        (random_rename, 5),
        (edit, 20),
        (delete, 10),
        (change_mode, 3),
        (file_to_dir, 3),
        (dir_to_file, 3),
    ];
    let shufflers_distribution = WeightedIndex::new(shufflers.iter().map(|(_, w)| w))?;
    for _ in 0..num_mutations {
        let index = shufflers_distribution.sample(rng);
        (shufflers[index].0)(dir, rng)?;
    }
    Ok(())
}
