use std::{
    path::{Path, PathBuf},
    thread::sleep,
    time::Duration,
};

use anyhow::Result;
use fs_err::{create_dir, read_dir, remove_dir_all, remove_file, rename, symlink_metadata, write};
use rammingen::term::debug;
use rand::{
    distributions::{Alphanumeric, DistString, WeightedIndex},
    prelude::Distribution,
    seq::SliceRandom,
    thread_rng, Rng,
};

use crate::is_ignored;

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

fn random_name() -> String {
    if thread_rng().gen_bool(0.1) {
        // ignored name
        "target".into()
    } else if thread_rng().gen_bool(0.1) {
        // ignored name
        format!("build_{}", thread_rng().gen_range(0..1000))
    } else {
        let name_len = thread_rng().gen_range(1..=10);
        Alphanumeric.sample_string(&mut thread_rng(), name_len)
    }
}

fn random_content() -> String {
    let content_len = thread_rng().gen_range(0..=30_000);
    Alphanumeric.sample_string(&mut thread_rng(), content_len)
}

pub fn choose_path(
    dir: &Path,
    allow_files: bool,
    allow_dirs: bool,
    allow_root: bool,
    allow_ignored: bool,
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
    Ok(paths.choose(&mut thread_rng()).cloned())
}

fn create(dir: &Path) -> Result<()> {
    let parent = choose_path(dir, false, true, true, true)?.unwrap();
    let path = parent.join(random_name());
    if path.exists() {
        return Ok(());
    }
    if thread_rng().gen_bool(0.1) {
        // dir
        create_dir(&path)?;
        debug(format!("created dir {}", path.display()));
    } else {
        // file
        write(&path, random_content())?;
        debug(format!("created file {}", path.display()));
    }
    Ok(())
}

fn file_to_dir(dir: &Path) -> Result<()> {
    let Some(path) = choose_path(dir, true, false, false, true)? else {
        return Ok(());
    };
    remove_file(&path)?;
    create_dir(&path)?;
    debug(format!("replaced file with dir {}", path.display()));
    Ok(())
}

fn dir_to_file(dir: &Path) -> Result<()> {
    let Some(path) = choose_path(dir, false, true, false, true)? else {
        return Ok(());
    };
    remove_dir_all(&path)?;
    write(&path, random_content())?;
    debug(format!("replaced dir with file {}", path.display()));
    Ok(())
}

fn random_rename(dir: &Path) -> Result<()> {
    let Some(from) = choose_path(dir, true, true, false, true)? else {
        return Ok(());
    };
    let to = if thread_rng().gen_bool(0.2) {
        choose_path(dir, false, true, true, true)?
            .unwrap()
            .join(random_name())
    } else {
        from.parent().unwrap().join(random_name())
    };
    if !to.exists() && !to.starts_with(&from) {
        rename(&from, &to)?;
        debug(format!("renamed {} -> {}", from.display(), to.display()));
    }
    Ok(())
}

fn edit(dir: &Path) -> Result<()> {
    let Some(path) = choose_path(dir, true, false, false, true)? else {
        return Ok(());
    };
    if symlink_metadata(&path)?.modified()?.elapsed()? < Duration::from_millis(50) {
        sleep(Duration::from_millis(50));
    }
    write(&path, random_content())?;
    //let new_modified = symlink_metadata(&path)?.modified()?;
    debug(format!(
        "edited file {}", // (modified: {:?} -> {:?})",
        path.display(),
        //   old_modified,
        //   new_modified
    ));
    Ok(())
}

fn change_mode(dir: &Path) -> Result<()> {
    #[cfg(target_family = "unix")]
    {
        use std::fs::Permissions;
        use std::os::unix::prelude::PermissionsExt;

        let Some(path) = choose_path(dir, true, false, false, true)? else {
            return Ok(());
        };
        let mode = [0o777, 0o774, 0o744, 0o700, 0o666, 0o664, 0o644, 0o600]
            .choose(&mut thread_rng())
            .unwrap();

        fs_err::set_permissions(&path, Permissions::from_mode(*mode))?;
        debug(format!(
            "changed mode of file {} to {:#o}",
            path.display(),
            mode
        ));
    }
    Ok(())
}

fn delete(dir: &Path) -> Result<()> {
    if thread_rng().gen_bool(0.1) {
        // dir
        let Some(path) = choose_path(dir, false, true, false, true)? else {
            return Ok(());
        };
        remove_dir_all(&path)?;
        debug(format!("removed dir {}", path.display()));
    } else {
        // file
        let Some(path) = choose_path(dir, true, false, false, true)? else {
            return Ok(());
        };
        remove_file(&path)?;
        debug(format!("removed file {}", path.display()));
    }
    Ok(())
}

type Shuffler = fn(dir: &Path) -> Result<()>;

pub fn shuffle(dir: &Path) -> Result<()> {
    let num_mutations = thread_rng().gen_range(1..=30);
    let shufflers: &[(Shuffler, i32)] = &[
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
        let index = shufflers_distribution.sample(&mut thread_rng());
        (shufflers[index].0)(dir)?;
    }
    Ok(())
}
