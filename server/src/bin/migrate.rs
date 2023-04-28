use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_url = &env::args().nth(1).expect("missing database url arg");
    rammingen_server::migrate(db_url).await?;
    Ok(())
}
