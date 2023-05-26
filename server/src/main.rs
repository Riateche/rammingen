use anyhow::Result;
use clap::Parser;
use rammingen_protocol::util::log_writer;
use rammingen_server::Config;
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
    let config_path = if let Some(path) = cli.config {
        path
    } else {
        default_config_dir()?.join("rammingen-server.conf")
    };
    let config = Config::parse(config_path)?;

    tracing_subscriber::fmt()
        .with_writer(Mutex::new(log_writer(config.log_file.as_deref())?))
        .with_env_filter(EnvFilter::try_new(&config.log_filter)?)
        .finish()
        .init();
    rammingen_server::run(config).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn default_config_dir() -> Result<PathBuf> {
    Ok("/etc".into())
}

// Windows: %APPDATA% (%USERPROFILE%\AppData\Roaming);
// macOS: $HOME/Library/Application Support
#[cfg(not(target_os = "linux"))]
fn default_config_dir() -> Result<PathBuf> {
    dirs::config_dir().ok_or_else(|| anyhow::anyhow!("failed to get config dir"))
}
