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

    #[arg(long, help = "Enable explicit Control API development mode")]
    pub dev: bool,

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_flag_enables_explicit_development_mode() {
        let cli = Cli::try_parse_from(["grass-control-api", "--dev"]).unwrap();

        assert!(cli.dev);
    }
}
