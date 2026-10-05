mod capture_replay;
mod cmd;
mod data_loader;
mod dispatch;
mod handle;
mod messages;
mod msgid;
mod packet;
mod progression;
mod proxy;
mod sequence;
mod server;
mod state;

use std::{path::PathBuf, sync::Arc};

use capture_replay::CaptureReplay;
use clap::Parser;
use common::{init_tracing, GAMESERVER, GAMESERVER_PORT};

#[derive(clap::Parser, Debug)]
#[command(about = "Local game server, TCP proxy, and decoded capture replay")]
struct Args {
    /// Forward traffic to this upstream server instead of emulating it.
    #[arg(long)]
    proxy: Option<String>,

    /// Replay decoded response groups from a proxy JSONL capture.
    #[arg(long, value_name = "CAPTURE.jsonl", conflicts_with = "proxy")]
    replay_capture: Option<PathBuf>,

    /// Local interface/address to listen on.
    #[arg(long, default_value_t = GAMESERVER)]
    bind: String,

    /// Local TCP port to listen on.
    #[arg(long, default_value_t = GAMESERVER_PORT)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let args = Args::parse();
    let listen_addr = format!("{}:{}", args.bind, args.port);

    if let Some(remote) = args.proxy {
        proxy::run_proxy(&remote, &listen_addr).await?;
    } else {
        let replay = args
            .replay_capture
            .map(|path| CaptureReplay::load(&path).map(Arc::new))
            .transpose()?;
        server::run_server_with_replay(&listen_addr, replay).await?;
    }

    Ok(())
}
