mod cli;
mod client;
mod protocol;
mod server;
mod tls;
mod transfer;

use crate::cli::{Cli, Command};
use anyhow::{bail, Result};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve { port, state_dir } => {
            server::run(port, &state_dir).await?;
        }

        Command::Connect { target, ca } => {
            let (endpoint, connection) = client::connect(&ca, &target).await?;

            println!("connected to {}", connection.remote_address());

            connection.close(0u32.into(), b"done");
            endpoint.wait_idle().await;
        }

        Command::Cp { src, dst, ca } => {
            let src_remote = client::is_remote(&src);
            let dst_remote = client::is_remote(&dst);

            if src_remote == dst_remote {
                bail!("exactly one of src and dst must be remote");
            }

            if src_remote {
                client::download(&ca, &src, std::path::Path::new(&dst)).await?;
            } else {
                client::upload(&ca, std::path::Path::new(&src), &dst).await?;
            }
        }
    }

    Ok(())
}
