use std::{fmt, str::FromStr};

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use serde::Serialize;
use serde::{de::Error, Deserialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ArchivePath(pub String);

impl ArchivePath {
    pub fn from_str_without_prefix(path: &str) -> Result<Self> {
        check_path(path)?;
        Ok(Self(path.into()))
    }

    pub fn join(&self, file_name: &str) -> Result<ArchivePath> {
        if file_name.is_empty() {
            bail!("file name cannot be empty");
        }
        if file_name.contains('/') {
            bail!("file name cannot contain '/'");
        }
        let s = format!("{}/{}", self.0, file_name);
        check_path(&s)?;
        Ok(Self(s))
    }

    pub fn parent(&self) -> Option<ArchivePath> {
        if self.0 == "/" {
            None
        } else {
            let pos = self.0.rfind('/').expect("any path must contain '/'");
            let parent = if pos == 0 { "/" } else { &self.0[..pos] };
            check_path(parent).expect("parent should always be valid");
            Some(Self(parent.into()))
        }
    }

    pub fn strip_prefix(&self, base: &ArchivePath) -> Option<&str> {
        self.0
            .strip_prefix(&base.0)
            .and_then(|prefix| prefix.strip_prefix('/'))
    }
}

#[test]
fn parent_path() {
    assert_eq!(ArchivePath::from_str("ar:/").unwrap().parent(), None);
    assert_eq!(
        ArchivePath::from_str("ar:/ab").unwrap().parent(),
        Some(ArchivePath::from_str("ar:/").unwrap())
    );
    assert_eq!(
        ArchivePath::from_str("ar:/ab/cd").unwrap().parent(),
        Some(ArchivePath::from_str("ar:/ab").unwrap())
    );
}

#[test]
fn strip_prefix() {
    fn p(s: &str) -> ArchivePath {
        ArchivePath::from_str_without_prefix(s).unwrap()
    }
    assert_eq!(p("/a/b/c/d").strip_prefix(&p("/a/b")), Some("c/d"));
    assert_eq!(p("/a1/b1/c1/d1").strip_prefix(&p("/a1/b1")), Some("c1/d1"));
    assert_eq!(p("/a/b/c/d").strip_prefix(&p("/a/b/c/d")), None);
    assert_eq!(p("/a/b/c/d").strip_prefix(&p("/d")), None);
}

impl<'de> Deserialize<'de> for ArchivePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        check_path(&s).map_err(D::Error::custom)?;
        Ok(Self(s))
    }
}

impl FromStr for ArchivePath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let path = s
            .strip_prefix("ar:")
            .ok_or_else(|| anyhow!("archive path must start with 'ar:'"))?;
        check_path(path)?;
        Ok(Self(path.into()))
    }
}

impl fmt::Display for ArchivePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ar:{}", self.0)
    }
}

fn check_path(path: &str) -> Result<()> {
    if path.contains("//") {
        bail!("path cannot contain '//'");
    }
    if !path.starts_with('/') {
        bail!("path must start with '/'");
    }
    if path != "/" && path.ends_with('/') {
        bail!("path must not end with '/'");
    }
    Ok(())
}
