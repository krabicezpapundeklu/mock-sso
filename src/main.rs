use std::path::{absolute, PathBuf};

use anyhow::Result;
use clap::Parser;

mod server;

#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(default_value = "0.0.0.0", long)]
    host: String,

    #[arg(default_value_t = 8080, long)]
    port: u16,

    #[arg(default_value = "key.pem", long)]
    key: PathBuf,

    #[arg(default_value = "cert.pem", long)]
    cert: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    server::start(
        &args.host,
        args.port,
        absolute(args.key)?.into_os_string(),
        absolute(args.cert)?.into_os_string(),
    )
    .await
}
