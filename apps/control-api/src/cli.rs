use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(
        long,
        env = "GWAPI_CONFIG",
        default_value = "config.toml",
        help = "Path to the Control API config file"
    )]
    pub config: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Run database migrations and exit")]
    Migrate,
}

impl Cli {
    pub fn config_path(&self) -> &str {
        &self.config
    }
}
