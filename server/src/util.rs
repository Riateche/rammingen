use {
    anyhow::{bail, Result},
    rammingen_protocol::credentials::AccessToken,
    sqlx::{query, query_scalar, PgPool},
};

/// Get names of configured sources (clients).
pub async fn sources(db: &PgPool) -> Result<Vec<String>> {
    query_scalar!("SELECT name FROM sources ORDER BY name")
        .fetch_all(db)
        .await
        .map_err(Into::into)
}

/// Add a source to the database.
pub async fn add_source(db: &PgPool, name: &str, access_token: &AccessToken) -> Result<()> {
    query!(
        "INSERT INTO sources (name, access_token) VALUES ($1, $2)",
        name,
        access_token.as_unmasked_str(),
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Change access token for an existing source.
pub async fn set_access_token(db: &PgPool, name: &str, access_token: &AccessToken) -> Result<()> {
    let rows = query!(
        "UPDATE sources SET access_token = $1 WHERE name = $2",
        access_token.as_unmasked_str(),
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

/// Apply migrations to the database.
pub async fn migrate(db: &PgPool) -> Result<()> {
    sqlx::migrate!().run(db).await?;
    Ok(())
}
