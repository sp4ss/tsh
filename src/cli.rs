use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "tsh",
    about = "tiny secure remote shell and file transfer over quic"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    #[command(name = "serve")]
    Serve {
        #[arg(short, long, default_value = "8080")]
        port: u16,

        #[arg(short, long, default_value = "./state")]
        state_dir: PathBuf,
    },

    #[command(name = "connect")]
    Connect {
        target: String,

        #[arg(short, long, default_value = "./state/server.der")]
        ca: PathBuf,
    },

    #[command(name = "cp")]
    Cp {
        src: String,

        dst: String,

        #[arg(short, long, default_value = "./state/server.der")]
        ca: PathBuf,
    },
}
