use {
    anyhow::Result,
    clap::Parser,
    rammingen_protocol::util::log_writer,
    rammingen_server::{config_path, Config},
    std::{path::PathBuf, sync::Mutex},
    tracing_subscriber::{util::SubscriberInitExt, EnvFilter},
};

#[derive(Debug, Parser)]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "File sync and backup server")]
pub struct Cli {
    /// Path to config.
    ///
    /// If omitted, default path is used:
    ///
    /// - /etc/rammingen-server.conf on Linux
    ///
    /// - $HOME/Library/Application Support/rammingen-server.conf on macOS
    ///
    /// - %APPDATA%\rammingen-server.conf on Windows
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
