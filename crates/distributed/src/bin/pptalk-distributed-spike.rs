use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pptalk_distributed::{DistributedLocator, VeilidBlobStore};

#[derive(Debug, Parser)]
#[command(about = "Cross-process Veilid viability probe for opaque pptalk payloads")]
struct Args {
    #[arg(long)]
    storage: PathBuf,
    #[arg(long, default_value = "spike")]
    namespace: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Publish { input: PathBuf, locator: PathBuf },
    Fetch { locator: PathBuf, output: PathBuf },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let store = VeilidBlobStore::start(&args.storage, &args.namespace)
        .await
        .context("start Veilid")?;
    match args.command {
        Command::Publish {
            input,
            locator: output,
        } => {
            let bytes =
                std::fs::read(&input).with_context(|| format!("read {}", input.display()))?;
            let locator = store.publish(&bytes).await.context("publish payload")?;
            let encoded = serde_json::to_vec_pretty(&locator)?;
            std::fs::write(&output, encoded)
                .with_context(|| format!("write {}", output.display()))?;
            println!("{}", output.display());
        }
        Command::Fetch { locator, output } => {
            let locator: DistributedLocator = serde_json::from_slice(
                &std::fs::read(&locator).with_context(|| format!("read {}", locator.display()))?,
            )?;
            let bytes = store.retrieve(&locator).await.context("retrieve payload")?;
            std::fs::write(&output, bytes)
                .with_context(|| format!("write {}", output.display()))?;
        }
    }
    store.shutdown().await;
    Ok(())
}
