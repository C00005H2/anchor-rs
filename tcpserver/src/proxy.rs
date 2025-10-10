use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    fs::OpenOptions,
};
use tracing::info;
use std::convert::TryFrom;
use serde_json::{json, Value};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{
    packet::*,
    msgid::MsgId,
    dispatch::dispatch_cmd,
};

use crypto::asset_setting::AssetSetting;
use common::{GAMESERVER, GAMESERVER_PORT};

#[derive(Debug, Clone, Serialize)]
struct PacketInfo {
    timestamp: DateTime<Utc>,
    direction: String, // "C->S" or "S->C"
    cmd: u32,
    name: String,
    payload_len: usize,
    decoded: Option<Value>,
}

#[derive(Debug, Serialize)]
struct RequestGroup {
    client_request: PacketInfo,
    server_responses: Vec<PacketInfo>,
}

enum PacketEvent {
    Client(PacketInfo),
    Server(PacketInfo),
}

/// Save a request+response group to JSONL
async fn save_request_group(
    group: &RequestGroup,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filename = format!("requests_{}.jsonl",
                           group.client_request.timestamp.format("%Y%m%d"));

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&filename)
        .await?;

    let group_json = json!({
        "timestamp": group.client_request.timestamp.to_rfc3339(),
        "client_request": {
            "cmd": group.client_request.cmd,
            "name": group.client_request.name,
            "payload_len": group.client_request.payload_len,
            "decoded": group.client_request.decoded
        },
        "server_responses": group.server_responses.iter().map(|resp| json!({
            "timestamp": resp.timestamp.to_rfc3339(),
            "cmd": resp.cmd,
            "name": resp.name,
            "payload_len": resp.payload_len,
            "decoded": resp.decoded
        })).collect::<Vec<_>>()
    });

    let json_string = serde_json::to_string(&group_json)?;
    file.write_all(format!("{}\n", json_string).as_bytes()).await?;
    file.flush().await?;

    info!("[SAVED] Request group with {} responses to {}",
        group.server_responses.len(), filename);

    Ok(())
}

fn is_ping_packet(cmd: u32) -> bool {
    cmd == 10000 || cmd == 10001 // CS_SYS_PING and SC_SYS_PING
}

/// Run the proxy server
pub async fn run_proxy(real_server: &str) -> anyhow::Result<()> {
    let listen_addr = format!("{}:{}", GAMESERVER, GAMESERVER_PORT);
    let key = AssetSetting::protocol_key();

    let listener = TcpListener::bind(&listen_addr).await?;
    info!("[*] Proxy listening on {}, forwarding to {}", listen_addr, real_server);

    loop {
        let (client, addr) = listener.accept().await?;
        info!("[+] Client connected: {}", addr);

        let server = TcpStream::connect(real_server).await?;
        info!("[+] Connected to real server {}", real_server);

        let (c_reader, c_writer) = client.into_split();
        let (s_reader, s_writer) = server.into_split();
        let key_c = key.clone();
        let key_s = key.clone();

        // Unified channel
        let (tx, mut rx) = mpsc::unbounded_channel::<PacketEvent>();

        // Saver task per connection
        tokio::spawn(async move {
            let mut current_group: Option<RequestGroup> = None;

            while let Some(event) = rx.recv().await {
                match event {
                    PacketEvent::Client(pkt) => {
                        if is_ping_packet(pkt.cmd) {
                            continue;
                        }
                        if let Some(group) = current_group.take() {
                            let _ = save_request_group(&group).await;
                        }
                        current_group = Some(RequestGroup {
                            client_request: pkt,
                            server_responses: Vec::new(),
                        });
                    }
                    PacketEvent::Server(pkt) => {
                        if is_ping_packet(pkt.cmd) {
                            continue;
                        }
                        if let Some(ref mut group) = current_group {
                            group.server_responses.push(pkt);
                        }
                    }
                }
            }

            if let Some(group) = current_group {
                let _ = save_request_group(&group).await;
            }
        });

        // Client → Server
        let tx_client = tx.clone();
        tokio::spawn(async move {
            let mut client_buffer = PacketBuffer::new(true);
            let mut tmp = [0u8; 4096];
            let mut s_writer = s_writer;
            let mut c_reader = c_reader;

            loop {
                match c_reader.read(&mut tmp).await {
                    Ok(0) => {
                        info!("[C->S] connection closed");
                        break;
                    }
                    Ok(n) => {
                        client_buffer.push_data(&tmp[..n]);
                        for pkt in client_buffer.drain_complete_packets() {
                            if let Some((cmd, dec)) = parse_client_packet(&pkt, &key_c) {
                                let name = MsgId::try_from(cmd)
                                    .map(|id| id.to_string())
                                    .unwrap_or_else(|_| format!("UNKNOWN({})", cmd));

                                info!("[C->S] cmd={} ({}) payload_len={}", cmd, name, dec.len());

                                let decoded_val = dispatch_cmd(cmd, &dec);

                                if let Some(ref val) = decoded_val {
                                    info!("[DECODED] {}", val);
                                }

                                let packet_info = PacketInfo {
                                    timestamp: Utc::now(),
                                    direction: "C->S".into(),
                                    cmd,
                                    name,
                                    payload_len: dec.len(),
                                    decoded: decoded_val,
                                };

                                let _ = tx_client.send(PacketEvent::Client(packet_info));
                            }

                            if let Err(e) = s_writer.write_all(&pkt).await {
                                info!("[C->S] write error: {}", e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        info!("[C->S] error: {}", e);
                        break;
                    }
                }
            }
        });

        // Server → Client
        let tx_server = tx.clone();
        tokio::spawn(async move {
            let mut server_buffer = PacketBuffer::new(false);
            let mut tmp = [0u8; 4096];
            let mut c_writer = c_writer;
            let mut s_reader = s_reader;

            loop {
                match s_reader.read(&mut tmp).await {
                    Ok(0) => {
                        info!("[S->C] connection closed");
                        break;
                    }
                    Ok(n) => {
                        server_buffer.push_data(&tmp[..n]);
                        for pkt in server_buffer.drain_complete_packets() {
                            if let Some((cmd, dec)) = parse_server_packet(&pkt, &key_s) {
                                let name = MsgId::try_from(cmd)
                                    .map(|id| id.to_string())
                                    .unwrap_or_else(|_| format!("UNKNOWN({})", cmd));

                                info!("[S->C] cmd={} ({}) payload_len={}", cmd, name, dec.len());

                                let decoded_val = dispatch_cmd(cmd, &dec);

                                if let Some(ref val) = decoded_val {
                                    info!("[DECODED] {}", val);
                                }

                                let packet_info = PacketInfo {
                                    timestamp: Utc::now(),
                                    direction: "S->C".into(),
                                    cmd,
                                    name,
                                    payload_len: dec.len(),
                                    decoded: decoded_val,
                                };

                                let _ = tx_server.send(PacketEvent::Server(packet_info));
                            }

                            if let Err(e) = c_writer.write_all(&pkt).await {
                                info!("[S->C] write error: {}", e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        info!("[S->C] error: {}", e);
                        break;
                    }
                }
            }
        });
    }
}
