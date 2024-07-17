use std::ffi::OsString;

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
    key: OsString,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    server::start(&args.host, args.port, args.key).await
}
