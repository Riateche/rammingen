use anyhow::{anyhow, bail, Result};
use itertools::Itertools;
use serde::{de::Error, Deserialize};
use std::{
    borrow::Cow,
    fmt::Display,
    path::{Path, PathBuf, MAIN_SEPARATOR, MAIN_SEPARATOR_STR},
    str::FromStr,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SanitizedLocalPath(pub String);

impl From<SanitizedLocalPath> for PathBuf {
    fn from(value: SanitizedLocalPath) -> Self {
        value.0.into()
    }
}

impl From<&SanitizedLocalPath> for PathBuf {
    fn from(value: &SanitizedLocalPath) -> Self {
        value.0.clone().into()
    }
}

impl AsRef<Path> for SanitizedLocalPath {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl Display for SanitizedLocalPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl SanitizedLocalPath {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let path = if path.try_exists()? {
            dunce::canonicalize(path)
                .map_err(|e| anyhow!("failed to canonicalize {:?}: {}", path, e))?
        } else {
            let parent = path
                .parent()
                .ok_or_else(|| anyhow!("unsupported path (couldn't get parent): {:?}", path))?;
            let file_name = path
                .file_name()
                .ok_or_else(|| anyhow!("unsupported path (couldn't get parent): {:?}", path))?;
            let parent = dunce::canonicalize(parent)
                .map_err(|e| anyhow!("failed to canonicalize {:?}: {}", parent, e))?;
            parent.join(file_name)
        };

        let str = path
            .to_str()
            .ok_or_else(|| anyhow!("unsupported path: {:?}", path))?;
        if str.is_empty() {
            bail!("path cannot be empty");
        }
        Ok(Self(str.into()))
    }

    pub fn join_file_name(&self, file_name: &str) -> Result<Self> {
        if file_name.is_empty() {
            bail!("file name cannot be empty");
        }
        if file_name.contains('/') {
            bail!("file name cannot contain '/'");
        }
        if file_name.contains('\\') {
            bail!("file name cannot contain '\\'");
        }
        let mut path = self.clone();
        path.0.push(MAIN_SEPARATOR);
        path.0.push_str(file_name);
        Ok(path)
    }

    pub fn join_path(&self, relative_path: &str) -> Result<Self> {
        let mut path = self.clone();
        path.0.push(MAIN_SEPARATOR);
        path.0.push_str(&fix_path_separator(relative_path));
        Ok(path)
    }

    pub fn file_name(&self) -> &str {
        self.0
            .split(MAIN_SEPARATOR)
            .rev()
            .next()
            .expect("cannot be empty")
    }

    pub fn parent(&self) -> Option<Self> {
        Path::new(&self.0).parent().map(|parent| {
            Self(
                parent
                    .to_str()
                    .expect("parent of sanitized path must be valid utf-8")
                    .into(),
            )
        })
    }
}

impl<'de> Deserialize<'de> for SanitizedLocalPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        Self::new(string).map_err(D::Error::custom)
    }
}

impl FromStr for SanitizedLocalPath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

fn fix_path_separator(path: &str) -> Cow<'_, str> {
    if MAIN_SEPARATOR == '/' {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(path.split('/').join(MAIN_SEPARATOR_STR))
    }
}
