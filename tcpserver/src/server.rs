use tokio::{
    io::AsyncReadExt,
    net::TcpListener,
};
use tracing::{info, error, warn};
use std::{convert::TryFrom, sync::Arc};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::{
    capture_replay::{redact_capture_value, CaptureReplay},
    packet::*,
    state::ConnectionContext,
    handle::dispatch_packet_with_replay,
    msgid::MsgId,
    dispatch::dispatch_cmd,
};

use crypto::asset_setting::AssetSetting;

pub async fn run_server(listen_addr: &str) -> anyhow::Result<()> {
    run_server_with_replay(listen_addr, None).await
}

pub async fn run_server_with_replay(
    listen_addr: &str,
    replay_archive: Option<Arc<CaptureReplay>>,
) -> anyhow::Result<()> {
    let key = AssetSetting::protocol_key();

    let listener = TcpListener::bind(listen_addr).await?;
    info!("[*] Game Server listening on {}", listen_addr);

    loop {
        let (client, addr) = listener.accept().await?;
        info!("[+] Client connected: {}", addr);

        let key_clone = key.to_string();
        let replay_clone = replay_archive.clone();
        let session_id = format!("temp_{}", chrono::Utc::now().timestamp());
        let ctx = Arc::new(Mutex::new(ConnectionContext::new(session_id)));

        tokio::spawn(async move {
            if let Err(e) = handle_client(client, ctx, key_clone, replay_clone).await {
                error!("[!] Client error: {}", e);
            }
        });
    }
}

async fn handle_client(
    client: TcpStream,
    ctx: Arc<Mutex<ConnectionContext>>,
    key: String,
    replay_archive: Option<Arc<CaptureReplay>>,
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

                        let mut decoded_val = dispatch_cmd(cmd_id, &decrypted_data);
                        if let Some(val) = decoded_val.as_mut() {
                            redact_capture_value(cmd_id, val);
                        }

                        let decoded_str = decoded_val
                            .as_ref()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "<no schema>".to_string());

                        let preview = if matches!(cmd_id, 11000 | 11007) {
                            "<redacted>".to_string()
                        } else {
                            hex_preview(&decrypted_data, 32)
                        };

                        info!(
                            cmd = cmd_id,
                            name = %name,
                            payload_len = decrypted_data.len(),
                            decoded = %decoded_str,
                            preview = %preview,
                            "Received client packet"
                        );

                        if let Err(e) = dispatch_packet_with_replay(
                            Arc::clone(&ctx),
                            cmd_id,
                            &decrypted_data,
                            &mut writer,
                            replay_archive.as_deref(),
                        )
                        .await
                        {
                            error!("[!] Command processing error for cmd={cmd_id} ({name}): {:#}", e);
                        }
                    } else {
                        warn!(
                            packet_len = pkt.len(),
                            preview = %hex_preview(&pkt, 32),
                            "[!] Failed to parse client packet"
                        );
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

