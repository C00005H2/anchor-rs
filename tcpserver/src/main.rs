mod proxy;
mod server;
mod msgid;
mod messages;
mod dispatch;
mod packet;
mod cmd;
mod state;
mod handle;
mod data_loader;

use clap::Parser;
use common::init_tracing;

#[derive(clap::Parser, Debug)]
struct Args {

    #[arg(long)]
    proxy: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let args = Args::parse();

    if let Some(remote) = args.proxy {
        proxy::run_proxy(&remote).await?;
    } else {
        server::run_server().await?;
    }

    Ok(())
}
