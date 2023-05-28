use anyhow::Result;
use clap::Parser;
use rammingen_protocol::util::log_writer;
use rammingen_server::{config_path, Config};
use std::{path::PathBuf, sync::Mutex};
use tracing_subscriber::{util::SubscriberInitExt, EnvFilter};

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(long)]
    pub config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = config_path(cli.config)?;
    let config = Config::parse(config_path)?;

    tracing_subscriber::fmt()
        .with_writer(Mutex::new(log_writer(config.log_file.as_deref())?))
        .with_env_filter(EnvFilter::try_new(&config.log_filter)?)
        .finish()
        .init();
    rammingen_server::run(config).await?;
    Ok(())
}
