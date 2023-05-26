use std::path::PathBuf;

use clap::{Parser, Subcommand};
use rammingen_server::{
    util::{add_source, generate_access_token, set_access_token, sources},
    Config,
};
use sqlx::PgPool;
#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(long)]
    pub config: PathBuf,
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    Sources,
    AddSource { name: String },
    UpdateAccessToken { name: String },
    Migrate,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::parse(&cli.config)?;
    let pool = PgPool::connect(&config.database_url).await?;
    match cli.command {
        Command::Sources => {
            let sources = sources(&pool).await?;
            if sources.is_empty() {
                println!("No configured sources.");
            }
            for source in sources {
                println!("{source}");
            }
        }
        Command::AddSource { name } => {
            let token = generate_access_token();
            add_source(&pool, &name, &token).await?;
            println!("Successfully added new source. New access token:\n{token}");
        }
        Command::UpdateAccessToken { name } => {
            let token = generate_access_token();
            set_access_token(&pool, &name, &token).await?;
            println!("Successfully updated access token. New access token:\n{token}");
        }
        Command::Migrate => {
            println!("Running migrations...");
            rammingen_server::util::migrate(&pool).await?;
            println!("Done");
        }
    };
    Ok(())
}
