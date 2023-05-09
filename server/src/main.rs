use anyhow::Result;
use rammingen_protocol::util::log_writer;
use rammingen_server::Config;
use std::{env, sync::Mutex};
use tracing_subscriber::{util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::args().nth(1).expect("missing config arg");
    let config: Config = json5::from_str(&fs_err::read_to_string(config_path)?)?;

    tracing_subscriber::fmt()
        .with_writer(Mutex::new(log_writer(config.log_file.as_deref())?))
        .with_env_filter(EnvFilter::try_new(&config.log_filter)?)
        .finish()
        .init();
    rammingen_server::run(config).await?;
    Ok(())
}
