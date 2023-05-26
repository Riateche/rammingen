use anyhow::{bail, Result};
use rand::{distributions::Alphanumeric, distributions::DistString, rngs::OsRng};
use sqlx::{query, PgPool};

pub async fn add_source(db: &PgPool, name: &str, access_token: &str) -> Result<()> {
    query!(
        "INSERT INTO sources (name, secret) VALUES ($1, $2)",
        name,
        access_token
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_access_token(db: &PgPool, name: &str, access_token: &str) -> Result<()> {
    let rows = query!(
        "UPDATE sources SET secret = $1 WHERE name = $2",
        access_token,
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

pub fn generate_access_token() -> String {
    Alphanumeric.sample_string(&mut OsRng, 64)
}

pub async fn migrate(db: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!().run(db).await?;
    Ok(())
}
