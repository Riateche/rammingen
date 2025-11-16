use {
    anyhow::{anyhow, bail, Result},
    serde::{de::Error, Deserialize, Serialize},
    std::{fmt, str::FromStr},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ArchivePath(String);

impl ArchivePath {
    pub fn from_str_without_prefix(path: &str) -> Result<Self> {
        check_path(path)?;
        Ok(Self(path.into()))
    }

    pub fn to_str_without_prefix(&self) -> &str {
        &self.0
    }

    pub fn join_one(&self, file_name: &str) -> Result<ArchivePath> {
        if file_name.is_empty() {
            bail!("file name cannot be empty");
        }
        if file_name.contains('/') {
            bail!("file name cannot contain '/'");
        }
        let s = if self.0 == "/" {
            format!("{}{}", self.0, file_name)
        } else {
            format!("{}/{}", self.0, file_name)
        };
        check_path(&s)?;
        Ok(Self(s))
    }

    pub fn join_multiple(&self, relative_archive_path: &str) -> Result<ArchivePath> {
        if relative_archive_path.is_empty() {
            bail!("relative_archive_path cannot be empty");
        }
        if relative_archive_path.contains("//") {
            bail!("relative_archive_path cannot contain '//'");
        }
        if relative_archive_path.starts_with('/') {
            bail!("relative_archive_path cannot start with '/'");
        }
        if relative_archive_path.ends_with('/') {
            bail!("relative_archive_path must not end with '/'");
        }

        let s = if self.0 == "/" {
            format!("{}{}", self.0, relative_archive_path)
        } else {
            format!("{}/{}", self.0, relative_archive_path)
        };
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
        if base.0 == "/" {
            self.0.strip_prefix(&base.0)
        } else {
            self.0
                .strip_prefix(&base.0)
                .and_then(|prefix| prefix.strip_prefix('/'))
        }
    }

    pub fn last_name(&self) -> Option<&str> {
        if self.0 == "/" {
            None
        } else {
            let pos = self.0.rfind('/').expect("any path must contain '/'");
            Some(&self.0[pos + 1..])
        }
    }
}

pub mod with_prefix {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ArchivePath, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        s.parse().map_err(D::Error::custom)
    }

    pub fn serialize<S>(value: &ArchivePath, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        value.to_string().serialize(serializer)
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

    assert_eq!(p("/a").strip_prefix(&p("/")), Some("a"));
    assert_eq!(p("/abc").strip_prefix(&p("/")), Some("abc"));
    assert_eq!(p("/a/b/c/d").strip_prefix(&p("/")), Some("a/b/c/d"));
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
        let mut path = s
            .strip_prefix("ar:")
            .ok_or_else(|| anyhow!("archive path must start with 'ar:/'"))?
            .to_string();
        if !path.starts_with('/') {
            bail!("archive path must start with 'ar:/'");
        }
        if path.ends_with('/') && path != "/" {
            path.pop();
        }
        check_path(&path)?;
        Ok(Self(path))
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EncryptedArchivePath(ArchivePath);

impl EncryptedArchivePath {
    pub fn from_encrypted_without_prefix(path: &str) -> Result<Self> {
        ArchivePath::from_str_without_prefix(path).map(Self)
    }

    pub fn to_str_without_prefix(&self) -> &str {
        self.0.to_str_without_prefix()
    }

    pub fn parent(&self) -> Option<EncryptedArchivePath> {
        self.0.parent().map(Self)
    }

    pub fn strip_prefix(&self, base: &EncryptedArchivePath) -> Option<&str> {
        self.0.strip_prefix(&base.0)
    }

    pub fn join_multiple(&self, relative_archive_path: &str) -> Result<EncryptedArchivePath> {
        self.0.join_multiple(relative_archive_path).map(Self)
    }
}

impl fmt::Display for EncryptedArchivePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "enar:{}", self.0 .0)
    }
}
