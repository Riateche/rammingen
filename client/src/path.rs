use {
    anyhow::{anyhow, bail, Context as _, Result},
    fs_err::{symlink_metadata, PathExt},
    serde::{de::Error, Deserialize, Serialize},
    std::{
        fmt::Display,
        path::{Component, Path, PathBuf},
        str::FromStr,
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SanitizedLocalPath(PathBuf);

impl From<SanitizedLocalPath> for PathBuf {
    fn from(value: SanitizedLocalPath) -> Self {
        value.0
    }
}

impl From<&SanitizedLocalPath> for PathBuf {
    fn from(value: &SanitizedLocalPath) -> Self {
        value.0.clone()
    }
}

impl AsRef<Path> for SanitizedLocalPath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl AsRef<[u8]> for SanitizedLocalPath {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl Display for SanitizedLocalPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", dunce::simplified(&self.0).display())
    }
}

fn canonicalize(path: &Path) -> Result<PathBuf> {
    // We intentionally ignore I/O errors on `exists()` because
    // it can fail with a "not a directory" error if a parent path is a file.
    if path.exists() {
        if symlink_metadata(path)?.is_symlink() {
            bail!("symlinks are not allowed; path: {:?}", path);
        }
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
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = canonicalize(path.as_ref())?;

        Self::new_without_canonicalize(path)
    }

    fn new_without_canonicalize(path: impl AsRef<Path>) -> Result<Self> {
        if path.as_ref().to_str().is_none() {
            bail!("unsupported path (not valid unicode): {:?}", path.as_ref());
        }

        Ok(Self(path.as_ref().into()))
    }

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
        Self::new_without_canonicalize(self.0.join(relative_path))
    }

    pub fn file_name(&self) -> Option<&str> {
        self.0.file_name().map(|s| {
            s.to_str()
                .expect("previously checked that it can be converted")
        })
    }

    pub fn parent(&self) -> Result<Option<Self>> {
        if let Some(parent) = self.0.parent() {
            Ok(Some(Self::new_without_canonicalize(parent)?))
        } else {
            Ok(None)
        }
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0
            .to_str()
            .expect("previously checked that it can be converted")
    }

    pub fn exists(&self) -> std::io::Result<bool> {
        self.0.fs_err_try_exists()
    }
}

impl<'de> Deserialize<'de> for SanitizedLocalPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        Self::new(path).map_err(D::Error::custom)
    }
}

impl FromStr for SanitizedLocalPath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}
