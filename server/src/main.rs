use anyhow::Result;
use rammingen_server::Config;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::args().nth(1).expect("missing config arg");
    let config: Config = json5::from_str(&fs_err::read_to_string(config_path)?)?;
    rammingen_server::run(config).await?;
    Ok(())
}
