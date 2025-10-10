use tokio::{
    io::AsyncReadExt,
    net::TcpListener,
};
use tracing::{info, error, warn};
use std::{convert::TryFrom, sync::Arc, time::Instant, collections::HashMap};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::{
    packet::*,
    state::ConnectionContext,
    handle::dispatch_packet,
    msgid::MsgId,
    dispatch::dispatch_cmd
};

use crypto::asset_setting::AssetSetting;
use common::{GAMESERVER, GAMESERVER_PORT};

pub async fn run_server() -> anyhow::Result<()> {
    let listen_addr = format!("{}:{}", GAMESERVER, GAMESERVER_PORT);
    let key = AssetSetting::protocol_key();

    let listener = TcpListener::bind(&listen_addr).await?;
    info!("[*] Game Server listening on {}", listen_addr);

    loop {
        let (client, addr) = listener.accept().await?;
        info!("[+] Client connected: {}", addr);

        let key_clone = key.to_string();
        let ctx = Arc::new(Mutex::new(ConnectionContext {
            player_id: None,
            session_id: format!("temp_{}", chrono::Utc::now().timestamp()),
            logged_in: false,
            last_heartbeat: Instant::now(),
            dialogue_state: HashMap::new(),
        }));

        tokio::spawn(async move {
            if let Err(e) = handle_client(client, ctx, key_clone).await {
                error!("[!] Client error: {}", e);
            }
        });
    }
}

async fn handle_client(
    mut client: TcpStream,
    ctx: Arc<Mutex<ConnectionContext>>,
    key: String,
) -> Result<(), anyhow::Error> {
    let (mut reader, mut writer) = client.into_split();
    let mut buffer = PacketBuffer::new(true); // client-to-server
    let mut tmp = [0u8; 4096];

    loop {
        match reader.read(&mut tmp).await {
            Ok(0) => {
                info!("[Client] Connection closed");
                break;
            }
            Ok(n) => {
                buffer.push_data(&tmp[..n]);
                let complete_packets = buffer.drain_complete_packets();

                for pkt in complete_packets {
                    if let Some((cmd_id, decrypted_data)) = parse_client_packet(&pkt, &key) {
                        let name = MsgId::try_from(cmd_id)
                            .map(|id| id.to_string())
                            .unwrap_or_else(|_| format!("UNKNOWN({})", cmd_id));

                        info!(
                            "[C->S] cmd={} ({}) payload_len={} preview={}",
                            cmd_id,
                            name,
                            decrypted_data.len(),
                            hex_preview(&decrypted_data, 32)
                        );

                        if let Some(val) = dispatch_cmd(cmd_id, &decrypted_data) {
                            info!("[DECODED] {}", val);
                        }

                        if let Err(e) =
                            dispatch_packet(Arc::clone(&ctx), cmd_id, &decrypted_data, &mut writer).await
                        {
                            error!("[!] Command processing error: {:#}", e);
                        }
                    } else {
                        warn!("[!] Failed to parse client packet");
                    }
                }
            }
            Err(e) => {
                error!("[Client] Read error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

