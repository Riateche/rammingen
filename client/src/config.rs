use {
    crate::{path::SanitizedLocalPath, rules::Rule},
    byte_unit::Byte,
    humantime::parse_duration,
    rammingen_protocol::{AccessToken, ArchivePath, EncryptionKey, serde_path_with_prefix},
    serde::{Deserialize, Serialize},
    std::{path::PathBuf, time::Duration},
    url::Url,
};

/// Configuration of a mount point.
///
/// Mount point is a pair of local path and archive path.
/// `rammingen sync` and `rammingen auto-sync` will perform
/// two-way synchronization of these paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountPoint {
    /// Local path that should be synchrionized.
    pub local_path: SanitizedLocalPath,
    /// Archive path for synchronization.
    ///
    /// Archive paths are universal for all clients connected to the same server.
    #[serde(with = "serde_path_with_prefix")]
    pub archive_path: ArchivePath,
    /// Exclude rules for this mount point.
    #[serde(default)]
    pub exclude: Vec<Rule>,
}

/// Rammingen client configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// Whether rammingen should use system keyring for access token and encryption key.
    ///
    /// If `true`, the secrets will be fetched from the keyring (Keychain on macOS,
    /// Windows Credential Store on Windows, DBus-based Secret Service on Linux).
    /// `access_token` and `encryption_key` should not be specified in the config.
    ///
    /// If `false`, `access_token` and `encryption_key` config fields will be used.
    /// On Linux and macOS, it's recommended to set `600` permissions for the config file.
    #[serde(default)]
    pub use_keyring: bool,
    /// Exclude rules that apply to all mount points.
    pub always_exclude: Vec<Rule>,
    /// List of mount points for `sync` command.
    pub mount_points: Vec<MountPoint>,
    /// Encryption key for encoding file content and metadata.
    ///
    /// Use `rammingen generate-encryption-key` to create a key. All clients of a user
    /// must use the same encryption key.
    ///
    /// Never share the encryption key with others. Never store it on the same server as
    /// your rammingen server.
    pub encryption_key: Option<EncryptionKey>,
    /// URL to your rammingen server.
    ///
    /// HTTPS must be used to ensure secure connection.
    pub server_url: Url,
    /// Access token for accessing your rammingen server.
    ///
    /// On the server, use `rammingen-admin add-source` to generate an access token.
    /// Each client should use a separate access token.
    ///
    /// Never share your access token with others.
    pub access_token: Option<AccessToken>,
    /// Interval between syncs when using `rammingen auto-sync`.
    ///
    /// Default value is `5min`.
    #[serde(with = "humantime_serde", default = "default_sync_interval")]
    pub sync_interval: Duration,
    /// Override path to the local metadata storage.
    #[serde(default)]
    pub local_db_path: Option<PathBuf>,
    /// Override log path.
    #[serde(default)]
    pub log_file: Option<PathBuf>,
    /// Override log filter in
    /// [tracing format](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html).
    #[serde(default = "default_log_filter")]
    pub log_filter: String,
    /// Warn when uploading files that are larger than the specified size.
    ///
    /// Examples: `"10 MB"`, `"10 MiB", "10 GB"`. Default value is `50 MB`.
    #[serde(default = "default_warn_about_files_larger_than")]
    pub warn_about_files_larger_than: Byte,
    /// Whether `rammingen sync` should send desktop notifications.
    ///
    /// Default is `true`. When enabled, `rammingen sync` and `rammingen auto-sync` will send
    /// error notifications when sync fails, and will also send statistics after a successful sync
    /// based on `desktop_notification_interval` config option.
    #[serde(default = "true_")]
    pub enable_desktop_notifications: bool,
    /// Minimal interval between desktop notifications about a successful sync.
    ///
    /// The message will contain accumulated sync statistics since the previous notification time.
    /// Default value is `1hour`. This setting doesn't affect error notifications - they will appear
    /// after every sync error.
    ///
    /// Has no effect if `enable_desktop_notifications` is set to `false`.
    #[serde(
        with = "humantime_serde",
        default = "default_desktop_notification_interval"
    )]
    pub desktop_notification_interval: Duration,
}

fn true_() -> bool {
    true
}

#[must_use]
#[inline]
#[expect(clippy::expect_used, reason = "hardcoded value is correct")]
pub fn default_sync_interval() -> Duration {
    parse_duration("5min").expect("incorrect hardcoded value")
}

#[must_use]
#[inline]
#[expect(clippy::expect_used, reason = "hardcoded value is correct")]
pub fn default_desktop_notification_interval() -> Duration {
    parse_duration("1hour").expect("incorrect hardcoded value")
}

#[must_use]
#[inline]
pub fn default_log_filter() -> String {
    "info".into()
}

#[must_use]
#[inline]
#[expect(clippy::expect_used, reason = "hardcoded value is correct")]
pub fn default_warn_about_files_larger_than() -> Byte {
    "50 MB".parse().expect("incorrect hardcoded value")
}
