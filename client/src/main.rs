use anyhow::{anyhow, Result};
use clap::Parser;
use rammingen::{
    cli::Cli,
    config::Config,
    term::{clear_status, error},
};

#[tokio::main]
async fn main() {
    let r = try_main().await;
    clear_status();
    if let Err(err) = r {
        error(format!("{:?}", err));
    }
}

async fn try_main() -> Result<()> {
    let cli = Cli::parse();

    let config_path = if let Some(config) = &cli.config {
        config.clone()
    } else {
        let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("cannot find config dir"))?;
        config_dir.join("rammingen.json5")
    };
    let config: Config = json5::from_str(&fs_err::read_to_string(config_path)?)?;
    rammingen::run(cli, config).await?;
    Ok(())
}
