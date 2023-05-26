use anyhow::{anyhow, Result};
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use clap::Parser;
use rammingen::{
    cli::{Cli, Command},
    config::{Config, EncryptionKey},
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
    if cli.command == Command::GenerateEncryptionKey {
        let key = EncryptionKey::generate();
        println!("{}", BASE64_URL_SAFE_NO_PAD.encode(key.0));
        return Ok(());
    }

    let config_path = if let Some(config) = &cli.config {
        config.clone()
    } else {
        let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("cannot find config dir"))?;
        config_dir.join("rammingen.conf")
    };
    let config: Config = json5::from_str(&fs_err::read_to_string(config_path)?)?;
    rammingen::run(cli, config).await?;
    Ok(())
}
