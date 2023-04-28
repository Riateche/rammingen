use anyhow::Result;
use clap::Parser;
use rammingen::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    rammingen::run(cli).await?;
    Ok(())
}
