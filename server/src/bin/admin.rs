use {
    anyhow::ensure,
    clap::{Args, Parser, Subcommand},
    rammingen_protocol::AccessToken,
    rammingen_server::{
        default_config_path,
        util::{add_source, migrate, set_access_token, sources, update_server_id},
        Config,
    },
    sqlx::PgPool,
    std::path::PathBuf,
};

#[derive(Debug, Parser)]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "File sync and backup utility")]
pub struct Cli {
    #[command(flatten)]
    pub config_specifier: ConfigSpecifier,
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
#[group(required = false, multiple = false)]
pub struct ConfigSpecifier {
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
    /// URL of the database, e.g.
    /// `postgres://$DB_USER:$DB_PASSWORD@$DB_HOST:$DB_PORT/$DB_NAME`
    #[clap(long)]
    pub database_url: Option<String>,
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
    /// Update server ID.
    UpdateServerId,
}

#[tokio::main]
#[expect(clippy::print_stdout, reason = "intended")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    ensure!(
        !(cli.config_specifier.config.is_some() && cli.config_specifier.database_url.is_some()),
        "cannot specify config and database url at the same time"
    );

    let database_url = if let Some(url) = cli.config_specifier.database_url {
        url
    } else {
        let config_path = if let Some(path) = cli.config_specifier.config {
            path
        } else {
            default_config_path()?
        };
        let config = Config::parse(&config_path)?;
        config.database_url
    };

    let pool = PgPool::connect(&database_url).await?;
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
            migrate(&pool).await?;
            println!("Done");
        }
        Command::UpdateServerId => {
            update_server_id(&pool).await?;
            println!("Done");
        }
    }
    Ok(())
}
