use anyhow::Result;
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use clap::Parser;
use tracing::error;

use rammingen_protocol::credentials::EncryptionKey;

use rammingen::{
    cli::{default_config_path, Cli, Command},
    config::Config,
    setup_logger,
};

#[tokio::main]
async fn main() {
    if let Err(err) = try_main().await {
        println!("{err:?}");
    }
}

async fn try_main() -> Result<()> {
    let cli = Cli::parse();
    if cli.command == Command::GenerateEncryptionKey {
        let key = EncryptionKey::generate();
        println!("{}", BASE64_URL_SAFE_NO_PAD.encode(key.get()));
        return Ok(());
    }

    let config_path = if let Some(config) = &cli.config {
        config.clone()
    } else {
        default_config_path()?
    };
    let config: Config = json5::from_str(&fs_err::read_to_string(config_path)?)?;
    setup_logger(config.log_file.clone(), config.log_filter.clone())?;
    if let Err(err) = rammingen::run(cli, config).await {
        error!("{err:?}");
    }
    Ok(())
}
