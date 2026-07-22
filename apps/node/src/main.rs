use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use pptalk_node::{MailboxStore, NodeConfig, router};

#[derive(Debug, Parser)]
#[command(about = "Optional self-hosted pptalk mailbox node")]
struct Args {
    #[arg(long, default_value = "0.0.0.0:9464")]
    listen: SocketAddr,
    #[arg(long, default_value = "./pptalk-node-data")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let store = MailboxStore::open(NodeConfig::at(&args.data_dir)).context("open mailbox store")?;
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    tracing::info!(listen = %args.listen, data_dir = %args.data_dir.display(), "pptalk node ready");
    axum::serve(listener, router(store))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
