use {
    anyhow::Result,
    base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD},
    clap::Parser,
    rammingen::{
        cli::{Cli, Command, default_config_path},
        config::Config,
        setup_logger,
        term::{StdoutTerm, set_term},
    },
    rammingen_protocol::EncryptionKey,
    tracing::error,
};

#[tokio::main]
#[expect(clippy::print_stdout, reason = "intended")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.command == Command::GenerateEncryptionKey {
        let key = EncryptionKey::generate()?;
        println!("{}", BASE64_URL_SAFE_NO_PAD.encode(key.get()));
        return Ok(());
    }

    let config_path = if let Some(config) = &cli.config {
        config.clone()
    } else {
        default_config_path()?
    };
    let config: Config = json5::from_str(&fs_err::read_to_string(config_path)?)?;
    set_term(Some(Box::new(StdoutTerm::new())));
    setup_logger(config.log_file.clone(), config.log_filter.clone())?;
    if let Err(err) = rammingen::run(cli.command, config, None).await {
        error!("{err:?}");
    }
    Ok(())
}
