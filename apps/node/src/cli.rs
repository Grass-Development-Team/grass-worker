use std::net::IpAddr;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[arg(
        long,
        env = "GWNODE_CONFIG",
        default_value = "config/node.toml",
        help = "Path to the Node config file"
    )]
    pub config: String,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(hide = true)]
    GitProxy {
        #[arg(env = "GRASS_GIT_TARGET_IP")]
        ip: IpAddr,
        #[arg(env = "GRASS_GIT_TARGET_PORT")]
        port: u16,
    },
}

impl Cli {
    pub fn config_path(&self) -> &str {
        &self.config
    }
}

pub async fn run_git_proxy(ip: IpAddr, port: u16) -> anyhow::Result<()> {
    let stream = tokio::net::TcpStream::connect((ip, port)).await?;
    let (mut remote_read, mut remote_write) = stream.into_split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let upload = tokio::io::copy(&mut stdin, &mut remote_write);
    let download = tokio::io::copy(&mut remote_read, &mut stdout);
    tokio::try_join!(upload, download)?;
    Ok(())
}
