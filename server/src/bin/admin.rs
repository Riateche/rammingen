use {
    clap::{Parser, Subcommand},
    rammingen_protocol::credentials::AccessToken,
    rammingen_server::{
        default_config_path,
        util::{add_source, set_access_token, sources},
        Config,
    },
    sqlx::PgPool,
    std::path::PathBuf,
};

#[derive(Debug, Parser)]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "File sync and backup utility")]
pub struct Cli {
    /// Path to server config.
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
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Displays names of all sources.
    Sources,
    /// Creates a new source.
    AddSource { name: String },
    /// Changes access token of an existing source.
    UpdateAccessToken { name: String },
    /// Intializes or updates database structure.
    Migrate,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_path = if let Some(path) = cli.config {
        path
    } else {
        default_config_path()?
    };
    let config = Config::parse(&config_path)?;
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
            let token = AccessToken::generate()?;
            add_source(&pool, &name, &token).await?;
            println!(
                "Successfully added new source. New access token:\n{}",
                token.as_unmasked_str(),
            );
        }
        Command::UpdateAccessToken { name } => {
            let token = AccessToken::generate()?;
            set_access_token(&pool, &name, &token).await?;
            println!(
                "Successfully updated access token. New access token:\n{}",
                token.as_unmasked_str(),
            );
        }
        Command::Migrate => {
            println!("Running migrations...");
            rammingen_server::util::migrate(&pool).await?;
            println!("Done");
        }
    };
    Ok(())
}
