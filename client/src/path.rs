use {
    anyhow::{Context as _, Result, anyhow, bail},
    cadd::prelude::{Cinto, IntoType},
    serde::{Deserialize, Serialize, de::Error},
    std::{
        fmt::{self, Display, Formatter},
        io,
        path::{Component, Path, PathBuf},
        str::FromStr,
    },
};

/// A constrained version of `PathBuf`.
///
/// `SanitizedLocalPath` maintains the following restrictions:
/// - Path is always valid UTF-8.
/// - Path is always absolute.
/// - Path doesn't contain `.` and `..`.
/// - Path uses the native OS path delimiter. '/' is not allowed on Windows, and
///   '\' is not allowed on Linux (even though it is technically a valid `Path`).
/// - Path doesn't contain repeated delimiters (`//`, `\\`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SanitizedLocalPath(PathBuf);

impl From<SanitizedLocalPath> for PathBuf {
    #[inline]
    fn from(value: SanitizedLocalPath) -> Self {
        value.0
    }
}

impl From<&SanitizedLocalPath> for PathBuf {
    #[inline]
    fn from(value: &SanitizedLocalPath) -> Self {
        value.0.clone()
    }
}

impl AsRef<Path> for SanitizedLocalPath {
    #[inline]
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl AsRef<str> for SanitizedLocalPath {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for SanitizedLocalPath {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl Display for SanitizedLocalPath {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", dunce::simplified(&self.0).display())
    }
}

/// Canonicalize a path that may not exist yet.
fn canonicalize(path: &Path) -> Result<PathBuf> {
    // We intentionally ignore I/O errors here because
    // it can fail with a "not a directory" error if a parent path is a file.
    if path.try_exists_nofollow().unwrap_or(false) {
        return Ok(fs_err::canonicalize(path)?);
    }

    // Only works if last component is `Component::Normal`.
    let file_name = path.file_name().ok_or_else(|| {
        anyhow!(
            "unsupported path (must end with file or dir name): {:?}",
            path
        )
    })?;
    // Should always work if `file_name()` works.
    let parent = path
        .parent()
        .with_context(|| format!("unsupported path (couldn't get parent): {:?}", path))?;

    Ok(canonicalize(parent)?.join(file_name))
}

impl SanitizedLocalPath {
    /// Returns the canonical representation of a path if it exists, or
    /// infers the canonical path based on canonical path to the closest existing parent path.
    #[inline]
    pub fn canonicalize(&self) -> Result<Self> {
        let path = canonicalize(&self.0)?;
        Self::new(path)
    }

    /// Converts `Path` to `SanitizedLocalPath` if it satisfies the requirements.
    #[inline]
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.is_absolute() {
            bail!(
                "relative path is not allowed in SanitizedLocalPath: {:?}",
                path
            );
        }
        let path_str = path.cinto_type::<&str>()?;
        if cfg!(target_os = "windows") {
            if path_str.contains('/') {
                bail!(
                    "'/' is not allowed in SanitizedLocalPath on Windows: {:?}",
                    path
                );
            }
            if path_str.contains(r"\\") {
                bail!(
                    r"'\\' is not allowed in SanitizedLocalPath on Windows: {:?}",
                    path
                );
            }
        } else {
            if path_str.contains("//") {
                bail!("'//' is not allowed in SanitizedLocalPath: {:?}", path);
            }
            if path_str.contains('\\') {
                bail!(
                    r"'\' is only allowed in SanitizedLocalPath on Windows: {:?}",
                    path
                );
            }
        }
        if path.components().any(|c| matches!(c, Component::CurDir)) {
            bail!("'/./' is not allowed in SanitizedLocalPath: {:?}", path);
        }
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            bail!("'/../' is not allowed in SanitizedLocalPath: {:?}", path);
        }
        Ok(Self(path.into()))
    }

    /// Join `self` with `relative_path`.
    ///
    /// `relative_path` must be relative and must conform to other constraints of `SanitizedLocalPath`.
    #[inline]
    pub fn join(&self, relative_path: impl AsRef<Path>) -> Result<Self> {
        let relative_path = relative_path.as_ref();
        if !relative_path.is_relative() {
            bail!("joining absolute path is not allowed: {:?}", relative_path);
        }
        if relative_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            bail!(
                "joining allowed only with normal path components: {:?}",
                relative_path
            );
        }
        Self::new(self.0.join(relative_path))
    }

    /// Returns the final component of the path, if there is one.
    #[must_use]
    #[inline]
    #[expect(
        clippy::expect_used,
        reason = "SanitizedLocalPath can only contain valid unicode paths"
    )]
    pub fn file_name(&self) -> Option<&str> {
        self.0
            .file_name()
            .map(|s| s.cinto().expect("non-unicode path in SanitizedLocalPath"))
    }

    /// Returns the parent path if possible.
    #[inline]
    pub fn parent(&self) -> Result<Option<Self>> {
        if let Some(parent) = self.0.parent() {
            Ok(Some(Self::new(parent)?))
        } else {
            Ok(None)
        }
    }

    #[must_use]
    #[inline]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    #[must_use]
    #[inline]
    #[expect(
        clippy::expect_used,
        reason = "SanitizedLocalPath can only contain valid unicode paths"
    )]
    pub fn as_str(&self) -> &str {
        self.0
            .as_path()
            .cinto()
            .expect("non-unicode path in SanitizedLocalPath")
    }

    #[inline]
    pub fn try_exists_nofollow(&self) -> io::Result<bool> {
        self.0.try_exists_nofollow()
    }
}

impl<'de> Deserialize<'de> for SanitizedLocalPath {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        Self::new(path)
            .and_then(|path| path.canonicalize())
            .map_err(D::Error::custom)
    }
}

impl FromStr for SanitizedLocalPath {
    type Err = anyhow::Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)?.canonicalize()
    }
}

pub trait PathExt {
    fn try_exists_nofollow(&self) -> io::Result<bool>;
}

impl PathExt for Path {
    #[inline]
    fn try_exists_nofollow(&self) -> io::Result<bool> {
        match fs_err::symlink_metadata(self) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}
