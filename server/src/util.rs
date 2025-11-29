use {
    anyhow::{bail, ensure, Result},
    rammingen_protocol::credentials::AccessToken,
    rand::distr::{Alphanumeric, SampleString},
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

const SERVER_ID_LENGTH: usize = 16;

pub fn generate_server_id() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), SERVER_ID_LENGTH)
}

/// Create a new server ID and write it to the database.
pub async fn update_server_id(db_pool: &PgPool) -> anyhow::Result<()> {
    let mut tx = db_pool.begin().await?;
    let rows = query_scalar!("SELECT server_id FROM server_id")
        .fetch_all(&mut *tx)
        .await?;
    ensure!(rows.len() < 2, "server_id table must contain only one row");
    let new_server_id = generate_server_id();
    if rows.is_empty() {
        println!("Initializing server ID");
        query!(
            "INSERT INTO server_id(server_id) VALUES ($1)",
            new_server_id
        )
        .execute(&mut *tx)
        .await?;
    } else {
        println!("Old server ID was {:?}", rows[0]);
        query!("UPDATE server_id SET server_id = $1", new_server_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    println!("New server ID is {new_server_id:?}");
    Ok(())
}

/// Apply migrations to the database.
pub async fn migrate(db: &PgPool) -> Result<()> {
    sqlx::migrate!().run(db).await?;
    Ok(())
}
