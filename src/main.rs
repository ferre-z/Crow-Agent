use clap::Parser;
use crow::cli::{run, Cli};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    run(args).await
}
