use std::path::{Path, PathBuf};

use anyhow::Result;
use fs_err::{create_dir, read_dir, remove_dir_all, remove_file, rename, symlink_metadata, write};
use rammingen::term::debug;
use rand::{
    distributions::{Alphanumeric, DistString, WeightedIndex},
    prelude::Distribution,
    seq::SliceRandom,
    thread_rng, Rng,
};

fn find_paths_inner(
    dir: &Path,
    allow_files: bool,
    allow_dirs: bool,
    allow_root: bool,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    if allow_dirs && allow_root {
        output.push(dir.into());
    }
    for entry in read_dir(dir)? {
        let entry = entry?.path();
        if symlink_metadata(&entry)?.is_file() {
            if allow_files {
                output.push(entry);
            }
        } else {
            find_paths_inner(&entry, allow_files, allow_dirs, true, output)?;
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

fn choose_path(
    dir: &Path,
    allow_files: bool,
    allow_dirs: bool,
    allow_root: bool,
) -> Result<Option<PathBuf>> {
    let mut paths = Vec::new();
    find_paths_inner(dir, allow_files, allow_dirs, allow_root, &mut paths)?;
    Ok(paths.choose(&mut thread_rng()).cloned())
}

fn create(dir: &Path) -> Result<()> {
    let parent = choose_path(dir, false, true, true)?.unwrap();
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

fn random_rename(dir: &Path) -> Result<()> {
    let Some(from) = choose_path(dir, true, true, false)? else {
        return Ok(());
    };
    let to = if thread_rng().gen_bool(0.2) {
        choose_path(dir, false, true, true)?
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
    let Some(path) = choose_path(dir, true, false, false)? else {
        return Ok(());
    };
    write(&path, random_content())?;
    debug(format!("edited file {}", path.display()));
    Ok(())
}

fn change_mode(dir: &Path) -> Result<()> {
    #[cfg(target_family = "unix")]
    {
        use std::fs::Permissions;
        use std::os::unix::prelude::PermissionsExt;

        let Some(path) = choose_path(dir, true, false, false)? else {
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
        let Some(path) = choose_path(dir, false, true, false)? else {
            return Ok(());
        };
        remove_dir_all(&path)?;
        debug(format!("removed dir {}", path.display()));
    } else {
        // file
        let Some(path) = choose_path(dir, true, false, false)? else {
            return Ok(());
        };
        remove_file(&path)?;
        debug(format!("removed file {}", path.display()));
    }
    Ok(())
}

type Shuffler = fn(dir: &Path) -> Result<()>;

pub fn shuffle(dir: &Path) -> Result<()> {
    let mut rng = thread_rng();
    let num_mutations = rng.gen_range(1..=30);
    let shufflers: &[(Shuffler, i32)] = &[
        (create, 10),
        (random_rename, 5),
        (edit, 10),
        (delete, 10),
        (change_mode, 3),
    ];
    let shufflers_distribution = WeightedIndex::new(shufflers.iter().map(|(_, w)| w))?;
    for _ in 0..num_mutations {
        let index = shufflers_distribution.sample(&mut thread_rng());
        (shufflers[index].0)(dir)?;
    }
    Ok(())
}
