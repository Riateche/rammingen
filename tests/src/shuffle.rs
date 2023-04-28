use std::path::{Path, PathBuf};

use anyhow::Result;
use fs_err::{create_dir, read_dir, remove_dir_all, remove_file, rename, symlink_metadata, write};
use rand::{
    distributions::{Alphanumeric, DistString},
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
    let name_len = thread_rng().gen_range(1..=10);
    Alphanumeric.sample_string(&mut thread_rng(), name_len)
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

fn shuffle(dir: &Path) -> Result<()> {
    let mut rng = thread_rng();
    let num_mutations = rng.gen_range(1..=30);
    for _ in 0..num_mutations {
        match rng.gen_range(0..=3) {
            // create
            0 => {
                let parent = choose_path(dir, false, true, true)?.unwrap();
                let name_len = rng.gen_range(1..=10);
                let path = parent.join(random_name());
                if rng.gen_bool(0.1) {
                    // dir
                    create_dir(&path)?;
                    println!("created dir {}", path.display());
                } else {
                    // file
                    let content_len = rng.gen_range(0..=30_000);
                    write(&path, random_content())?;
                    println!("created file {}", path.display());
                }
            }
            // rename
            1 => {
                let Some(from) = choose_path(dir, true, true, false)? else {
                    continue;
                };
                let to = if rng.gen_bool(0.2) {
                    // TODO: forbid destination inside source
                    choose_path(dir, false, true, true)?
                        .unwrap()
                        .join(random_name())
                } else {
                    from.parent().unwrap().join(random_name())
                };
                rename(&from, &to)?;
                println!("renamed {} -> {}", from.display(), to.display());
            }
            // edit
            2 => {
                let Some(path) = choose_path(dir, true, false, false)? else {
                    continue;
                };
                write(&path, random_content())?;
                println!("edited file {}", path.display());
            }
            // delete
            3 => {
                if rng.gen_bool(0.1) {
                    // dir
                    let Some(path) = choose_path(dir, false, true, false)? else {
                        continue;
                    };
                    remove_file(&path)?;
                    println!("removed file {}", path.display());
                } else {
                    // file
                    let Some(path) = choose_path(dir, true, false, false)? else {
                        continue;
                    };
                    remove_dir_all(&path)?;
                    println!("removed dir {}", path.display());
                }
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}
