use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(
        long,
        env = "GWNODE_CONFIG",
        default_value = "config/node.toml",
        help = "Path to the Node config file"
    )]
    pub config: String,
}

impl Cli {
    pub fn config_path(&self) -> &str {
        &self.config
    }
}
