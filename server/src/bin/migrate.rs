use std::env;

use sqlx::PgPool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = PgPool::connect(&env::args().nth(1).expect("missing database url arg")).await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(())
}
