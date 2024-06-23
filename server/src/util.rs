use anyhow::{bail, Result};
use sqlx::{query, query_scalar, PgPool};
use std::path::PathBuf;

use rammingen_protocol::credentials::AccessToken;

pub async fn sources(db: &PgPool) -> Result<Vec<String>> {
    query_scalar!("SELECT name FROM sources ORDER BY name")
        .fetch_all(db)
        .await
        .map_err(Into::into)
}

pub async fn add_source(db: &PgPool, name: &str, access_token: &AccessToken) -> Result<()> {
    query!(
        "INSERT INTO sources (name, access_token) VALUES ($1, $2)",
        name,
        access_token.as_ref(),
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_access_token(db: &PgPool, name: &str, access_token: &AccessToken) -> Result<()> {
    let rows = query!(
        "UPDATE sources SET access_token = $1 WHERE name = $2",
        access_token.as_ref(),
        name,
    )
    .execute(db)
    .await?
    .rows_affected();

    if rows == 0 {
        bail!("source not found");
    }
    Ok(())
}

pub async fn migrate(db: &PgPool) -> Result<()> {
    sqlx::migrate!().run(db).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn default_config_dir() -> Result<PathBuf> {
    Ok("/etc".into())
}

// Windows: %APPDATA% (%USERPROFILE%\AppData\Roaming);
// macOS: $HOME/Library/Application Support
#[cfg(not(target_os = "linux"))]
pub fn default_config_dir() -> Result<PathBuf> {
    dirs::config_dir().ok_or_else(|| anyhow::anyhow!("failed to get config dir"))
}
