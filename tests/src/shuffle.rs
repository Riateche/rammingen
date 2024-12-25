use std::{
    path::{Path, PathBuf},
    thread::sleep,
    time::Duration,
};

use anyhow::Result;
use fs_err::{create_dir, read_dir, remove_dir_all, remove_file, rename, symlink_metadata, write};
use rand::{
    distributions::{Alphanumeric, DistString, WeightedIndex},
    prelude::Distribution,
    seq::SliceRandom,
    Rng,
};
use tracing::debug;

use crate::{diff::is_leftover_dir_with_ignored_files, is_ignored};

fn find_paths_inner(
    dir: &Path,
    allow_files: bool,
    allow_dirs: bool,
    allow_root: bool,
    allow_ignored: bool,
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
        if symlink_metadata(&entry)?.is_file() {
            if allow_files {
                output.push(entry);
            }
        } else {
            find_paths_inner(&entry, allow_files, allow_dirs, true, allow_ignored, output)?;
            if allow_dirs {
                output.push(entry);
            }
        }
    }

    Ok(())
}

pub fn random_name(allow_ignored: bool, rng: &mut impl Rng) -> String {
    if allow_ignored && rng.gen_bool(0.1) {
        // ignored name
        "target".into()
    } else if allow_ignored && rng.gen_bool(0.1) {
        // ignored name
        format!("build_{}", rng.gen_range(0..1000))
    } else {
        let name_len = rng.gen_range(1..=10);
        Alphanumeric.sample_string(rng, name_len)
    }
}

pub fn random_content(rng: &mut impl Rng) -> String {
    let content_len = rng.gen_range(0..=3_000_000);
    Alphanumeric.sample_string(rng, content_len)
}

pub fn choose_path(
    dir: &Path,
    allow_files: bool,
    allow_dirs: bool,
    allow_root: bool,
    allow_ignored: bool,
    rng: &mut impl Rng,
) -> Result<Option<PathBuf>> {
    let mut paths = Vec::new();
    find_paths_inner(
        dir,
        allow_files,
        allow_dirs,
        allow_root,
        allow_ignored,
        &mut paths,
    )?;
    Ok(paths.choose(rng).cloned())
}

fn create(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let parent = choose_path(dir, false, true, true, true, rng)?.unwrap();
    if is_leftover_dir_with_ignored_files(&parent)? {
        return Ok(());
    }
    let path = parent.join(random_name(true, rng));
    if path.exists() {
        return Ok(());
    }
    if rng.gen_bool(0.1) {
        // dir
        create_dir(&path)?;
        debug!("created dir {}", path.display());
    } else {
        // file
        write(&path, random_content(rng))?;
        debug!("created file {}", path.display());
    }
    Ok(())
}

fn file_to_dir(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let Some(path) = choose_path(dir, true, false, false, true, rng)? else {
        return Ok(());
    };
    remove_file(&path)?;
    create_dir(&path)?;
    debug!("replaced file with dir {}", path.display());
    Ok(())
}

fn dir_to_file(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let Some(path) = choose_path(dir, false, true, false, true, rng)? else {
        return Ok(());
    };
    remove_dir_all(&path)?;
    write(&path, random_content(rng))?;
    debug!("replaced dir with file {}", path.display());
    Ok(())
}

fn random_rename(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let Some(from) = choose_path(dir, true, true, false, true, rng)? else {
        return Ok(());
    };
    let to = if rng.gen_bool(0.2) {
        choose_path(dir, false, true, true, true, rng)?
            .unwrap()
            .join(random_name(true, rng))
    } else {
        from.parent().unwrap().join(random_name(true, rng))
    };
    if !to.exists() && !to.starts_with(&from) {
        rename(&from, &to)?;
        debug!("renamed {} -> {}", from.display(), to.display());
    }
    Ok(())
}

fn edit(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    let Some(path) = choose_path(dir, true, false, false, true, rng)? else {
        return Ok(());
    };
    if symlink_metadata(&path)?.modified()?.elapsed()? < Duration::from_millis(50) {
        sleep(Duration::from_millis(50));
    }
    write(&path, random_content(rng))?;
    //let new_modified = symlink_metadata(&path)?.modified()?;
    debug!("edited file {}", path.display());
    Ok(())
}

fn change_mode(_dir: &Path, rng: &mut impl Rng) -> Result<()> {
    #[cfg(target_family = "unix")]
    {
        use std::fs::Permissions;
        use std::os::unix::prelude::PermissionsExt;

        let Some(path) = choose_path(_dir, true, false, false, true, rng)? else {
            return Ok(());
        };
        let mode = [0o777, 0o774, 0o744, 0o700, 0o666, 0o664, 0o644, 0o600]
            .choose(rng)
            .unwrap();

        fs_err::set_permissions(&path, Permissions::from_mode(*mode))?;
        debug!("changed mode of file {} to {:#o}", path.display(), mode);
    }
    Ok(())
}

fn delete(dir: &Path, rng: &mut impl Rng) -> Result<()> {
    if rng.gen_bool(0.1) {
        // dir
        let Some(path) = choose_path(dir, false, true, false, true, rng)? else {
            return Ok(());
        };
        remove_dir_all(&path)?;
        debug!("removed dir {}", path.display());
    } else {
        // file
        let Some(path) = choose_path(dir, true, false, false, true, rng)? else {
            return Ok(());
        };
        remove_file(&path)?;
        debug!("removed file {}", path.display());
    }
    Ok(())
}

type Shuffler<R> = fn(dir: &Path, &mut R) -> Result<()>;

pub fn shuffle<R: Rng>(dir: &Path, rng: &mut R) -> Result<()> {
    let num_mutations = rng.gen_range(1..=30);
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
