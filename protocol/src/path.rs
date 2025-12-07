use {
    anyhow::{Context as _, Result, bail},
    serde::{Deserialize, Serialize, de::Error},
    std::{fmt, str::FromStr},
};

/// Path in the virtual archive filesystem (`ar:/...`).
///
/// Archive path maintains the following constraints:
///
/// - Path is always valid UTF-8.
/// - Segment separator is always `/`.
/// - Path is always absolute and starts with `/`.
/// - Trailing `/` is not allowed.
/// - `.` and `..` components are not allowed.
/// - Empty segments (`//`) are not allowed.
///
/// Internal value is stored as a string without `ar:` prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ArchivePath(String);

impl ArchivePath {
    /// Create `ArchivePath` from path without `ar:` prefix.
    ///
    /// `path` should conform to `ArchivePath`'s constraints.
    #[inline]
    pub fn from_str_without_prefix(path: &str) -> Result<Self> {
        check_path(path)?;
        Ok(Self(path.into()))
    }

    /// Show the path without `ar:` prefix.
    #[must_use]
    #[inline]
    pub fn to_str_without_prefix(&self) -> &str {
        &self.0
    }

    /// Construct a new `ArchivePath` from parent directory path `self`
    /// and `file_name`.
    ///
    /// `file_name` should conform to `ArchivePath`'s constraints.
    #[inline]
    pub fn join_one(&self, file_name: &str) -> Result<ArchivePath> {
        if file_name.is_empty() {
            bail!("file name cannot be empty");
        }
        if file_name.contains('/') {
            bail!("file name cannot contain '/'");
        }
        if file_name == "." {
            bail!("file name cannot be \".\"");
        }
        if file_name == ".." {
            bail!("file name cannot be \"..\"");
        }
        let s = if self.0 == "/" {
            format!("{}{}", self.0, file_name)
        } else {
            format!("{}/{}", self.0, file_name)
        };
        check_path(&s)?;
        Ok(Self(s))
    }

    /// Construct a new `ArchivePath` from parent directory path `self`
    /// and a relative path inside the directory.
    ///
    /// `relative_archive_path` should conform to `ArchivePath`'s constraints.
    #[inline]
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
        if relative_archive_path.split('/').any(|part| part == ".") {
            bail!("relative_archive_path must not contain \".\" component");
        }
        if relative_archive_path.split('/').any(|part| part == "..") {
            bail!("relative_archive_path must not contain \"..\" component");
        }

        let s = if self.0 == "/" {
            format!("{}{}", self.0, relative_archive_path)
        } else {
            format!("{}/{}", self.0, relative_archive_path)
        };
        check_path(&s)?;
        Ok(Self(s))
    }

    /// Returns parent path, or `None` if this is the root path.
    #[must_use]
    #[expect(
        clippy::expect_used,
        clippy::unwrap_in_result,
        reason = "relies on previously checked invariants"
    )]
    #[inline]
    pub fn parent(&self) -> Option<ArchivePath> {
        if self.0 == "/" {
            None
        } else {
            let (mut parent, _filename) =
                self.0.rsplit_once('/').expect("any path must contain '/'");
            if parent.is_empty() {
                parent = "/";
            }
            check_path(parent).expect("parent should always be valid");
            Some(Self(parent.into()))
        }
    }

    /// Returns relative path from `base` to `self`, or `None` if `base` does not contain `self`.
    #[must_use]
    #[inline]
    pub fn strip_prefix(&self, base: &ArchivePath) -> Option<&str> {
        if base.0 == "/" {
            self.0.strip_prefix(&base.0)
        } else {
            self.0.strip_prefix(&base.0)?.strip_prefix('/')
        }
    }

    /// Returns last component of the path, or `None` if this is the root path.
    #[must_use]
    #[expect(
        clippy::expect_used,
        clippy::unwrap_in_result,
        reason = "relies on previously checked invariants"
    )]
    #[inline]
    pub fn last_name(&self) -> Option<&str> {
        if self.0 == "/" {
            None
        } else {
            let (_parent, last_name) = self.0.rsplit_once('/').expect("any path must contain '/'");
            Some(last_name)
        }
    }
}

/// Serialize and deserialize `ArchivePath` with `ar:` prefix.
pub mod with_prefix {
    use {
        crate::ArchivePath,
        serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error},
    };

    #[inline]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<ArchivePath, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        s.parse().map_err(D::Error::custom)
    }

    #[inline]
    pub fn serialize<S>(value: &ArchivePath, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ArchivePath {
    #[inline]
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

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut path = s
            .strip_prefix("ar:")
            .context("archive path must start with 'ar:/'")?
            .to_owned();
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
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ar:{}", self.0)
    }
}

/// Check if `path` is a valid `ArchivePath` (without `ar:` prefix).
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
    if path.split('/').any(|part| part == ".") {
        bail!("path must not contain \".\" component");
    }
    if path.split('/').any(|part| part == "..") {
        bail!("path must not contain \"..\" component");
    }
    Ok(())
}

/// Encrypted value of an `ArchivePath` (`enar:/...`).
///
/// This is the representation of `ArchivePath` available to the server.
/// `EncryptedArchivePath` is constructed by encrypting each component of `ArchivePath`
/// and joining them with `/`. For example:
///
/// - Archive path: `ar:/documents/pictures`
/// - Encrypted archive path: `enar:/KDmEW3xlU7jl_Z0jhnadnUnbba66BG9FnR/JrnKe8AVLAb2h5wEIuAlytHA`
///
/// (Actual value depends on the encryption key used by the client.)
///
/// The encryption result is deterministic, so the encrypted paths maintain the same
/// parent-child relationships as the original paths used to create them.
///
/// `EncryptedArchivePath` has the same value constraints as `ArchivePath`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EncryptedArchivePath(ArchivePath);

impl EncryptedArchivePath {
    /// Create `EncryptedArchivePath` from path without `enar:` prefix.
    ///
    /// `path` should conform to `ArchivePath`'s constraints.
    #[inline]
    pub fn from_encrypted_without_prefix(path: &str) -> Result<Self> {
        ArchivePath::from_str_without_prefix(path).map(Self)
    }

    /// Show the path without `enar:` prefix.
    #[must_use]
    #[inline]
    pub fn to_str_without_prefix(&self) -> &str {
        self.0.to_str_without_prefix()
    }

    /// Returns parent path, or `None` if this is the root path.
    #[must_use]
    #[inline]
    pub fn parent(&self) -> Option<EncryptedArchivePath> {
        self.0.parent().map(Self)
    }

    /// Returns relative path from `base` to `self`, or `None` if `base` does not contain `self`.
    #[must_use]
    #[inline]
    pub fn strip_prefix(&self, base: &EncryptedArchivePath) -> Option<&str> {
        self.0.strip_prefix(&base.0)
    }

    /// Construct a new `ArchivePath` from parent directory path `self`
    /// and a relative path inside the directory.
    ///
    /// `relative_archive_path` should conform to `ArchivePath`'s constraints.
    #[inline]
    pub fn join_multiple(&self, relative_archive_path: &str) -> Result<EncryptedArchivePath> {
        self.0.join_multiple(relative_archive_path).map(Self)
    }
}

impl fmt::Display for EncryptedArchivePath {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "enar:{}", self.0.0)
    }
}

#[cfg(test)]
mod tests {
    use {crate::ArchivePath, std::str::FromStr};

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
}
