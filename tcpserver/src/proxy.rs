use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::convert::TryFrom;
use tokio::{
    fs::OpenOptions,
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tracing::{error, info, warn};

use crate::{
    capture_replay::redact_capture_value,
    dispatch::dispatch_cmd,
    msgid::MsgId,
    packet::{parse_client_packet, parse_server_packet, PacketBuffer},
};

use crypto::asset_setting::AssetSetting;

const EVENT_CHANNEL_CAPACITY: usize = 128;

/// Undecoded payloads are recorded up to this size so the message catalogue can
/// be extended from a later capture.
const MAX_UNDECODED_HEX_BYTES: usize = 1024;

#[derive(Debug, Clone, Serialize)]
struct PacketInfo {
    timestamp: DateTime<Utc>,
    direction: String,
    cmd: u32,
    name: String,
    payload_len: usize,
    decoded: Option<Value>,
    /// Raw bytes of a server message with no known schema; `None` otherwise.
    payload_hex: Option<String>,
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

/// Append one request/response group to that day's JSONL capture file.
async fn save_request_group(group: &RequestGroup) -> anyhow::Result<()> {
    let filename = format!(
        "requests_{}.jsonl",
        group.client_request.timestamp.format("%Y%m%d")
    );

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&filename)
        .await
        .with_context(|| format!("could not open capture file {filename}"))?;

    let group_json = json!({
        "timestamp": group.client_request.timestamp.to_rfc3339(),
        "client_request": packet_json(&group.client_request),
        "server_responses": group
            .server_responses
            .iter()
            .map(packet_json)
            .collect::<Vec<_>>()
    });

    let json_string = serde_json::to_string(&group_json)?;
    file.write_all(format!("{json_string}\n").as_bytes()).await?;
    file.flush().await?;

    info!(
        responses = group.server_responses.len(),
        file = %filename,
        "Saved packet request group"
    );
    Ok(())
}

fn is_ping_packet(cmd: u32) -> bool {
    cmd == 10000 || cmd == 10001
}

/// Serialize one packet for a capture row. The optional raw payload of
/// messages without a known schema is only present when it was recorded.
fn packet_json(packet: &PacketInfo) -> Value {
    let mut object = serde_json::Map::new();
    object.insert(
        "timestamp".to_owned(),
        json!(packet.timestamp.to_rfc3339()),
    );
    object.insert("cmd".to_owned(), json!(packet.cmd));
    object.insert("name".to_owned(), json!(packet.name));
    object.insert("payload_len".to_owned(), json!(packet.payload_len));
    object.insert("decoded".to_owned(), json!(packet.decoded));
    if let Some(payload_hex) = &packet.payload_hex {
        object.insert("payload_hex".to_owned(), json!(payload_hex));
    }
    Value::Object(object)
}

fn hex_encode(data: &[u8], limit: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(data.len().min(limit) * 2);
    for byte in data.iter().take(limit) {
        text.push(HEX[(*byte >> 4) as usize] as char);
        text.push(HEX[(*byte & 0x0f) as usize] as char);
    }
    if data.len() > limit {
        text.push_str("...");
    }
    text
}

/// Run a TCP packet-inspection proxy. Upstream failures are isolated to the
/// affected client and do not stop accepting new connections.
pub async fn run_proxy(real_server: &str, listen_addr: &str) -> anyhow::Result<()> {
    let key = AssetSetting::protocol_key().to_owned();
    let listener = TcpListener::bind(listen_addr).await?;
    info!(
        "[*] Proxy listening on {}, forwarding to {}",
        listen_addr, real_server
    );

    loop {
        let (client, addr) = listener.accept().await?;
        info!("[+] Client connected: {}", addr);
        let upstream = real_server.to_owned();
        let key = key.clone();

        tokio::spawn(async move {
            match TcpStream::connect(&upstream).await {
                Ok(server) => {
                    info!(client = %addr, upstream = %upstream, "Connected proxy session");
                    if let Err(error) = handle_connection(client, server, &key).await {
                        error!(client = %addr, error = %error, "Proxy session failed");
                    }
                }
                Err(error) => {
                    error!(client = %addr, upstream = %upstream, error = %error, "Could not connect to upstream server");
                }
            }
        });
    }
}

async fn handle_connection(
    client: TcpStream,
    server: TcpStream,
    key: &str,
) -> anyhow::Result<()> {
    client.set_nodelay(true)?;
    server.set_nodelay(true)?;

    let (client_reader, client_writer) = client.into_split();
    let (server_reader, server_writer) = server.into_split();
    let (tx, rx) = mpsc::channel::<PacketEvent>(EVENT_CHANNEL_CAPACITY);
    let client_tx = tx.clone();
    let server_tx = tx.clone();

    let saver = tokio::spawn(save_events(rx));

    let client_to_server = async move {
        let mut reader = client_reader;
        let mut writer = server_writer;
        let mut buffer = PacketBuffer::new(true);
        let mut tmp = [0u8; 4096];

        loop {
            match reader.read(&mut tmp).await {
                Ok(0) => {
                    let _ = writer.shutdown().await;
                    info!("[C->S] client closed its write side");
                    break;
                }
                Ok(n) => {
                    buffer.push_data(&tmp[..n]);
                    for packet in buffer.drain_complete_packets() {
                        if let Some((cmd, decoded)) = parse_client_packet(&packet, key) {
                            let packet_info = make_packet_info("C->S", cmd, &decoded);
                            log_packet(&packet_info);
                            let _ = client_tx.send(PacketEvent::Client(packet_info)).await;
                        } else {
                            warn!(packet_len = packet.len(), "Could not decode client packet; forwarding unchanged");
                        }

                        writer.write_all(&packet).await.context("forwarding client packet")?;
                    }
                }
                Err(error) => {
                    let _ = writer.shutdown().await;
                    return Err(anyhow::Error::new(error).context("reading from client"));
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    };

    let server_to_client = async move {
        let mut reader = server_reader;
        let mut writer = client_writer;
        let mut buffer = PacketBuffer::new(false);
        let mut tmp = [0u8; 4096];

        loop {
            match reader.read(&mut tmp).await {
                Ok(0) => {
                    let _ = writer.shutdown().await;
                    info!("[S->C] upstream closed its write side");
                    break;
                }
                Ok(n) => {
                    buffer.push_data(&tmp[..n]);
                    for packet in buffer.drain_complete_packets() {
                        if let Some((cmd, decoded)) = parse_server_packet(&packet, key) {
                            let packet_info = make_packet_info("S->C", cmd, &decoded);
                            log_packet(&packet_info);
                            let _ = server_tx.send(PacketEvent::Server(packet_info)).await;
                        } else {
                            warn!(packet_len = packet.len(), "Could not decode server packet; forwarding unchanged");
                        }

                        writer.write_all(&packet).await.context("forwarding server packet")?;
                    }
                }
                Err(error) => {
                    let _ = writer.shutdown().await;
                    return Err(anyhow::Error::new(error).context("reading from upstream server"));
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    };

    // Preserve TCP half-close semantics: after one direction reaches EOF, the
    // other direction can still drain any final response before it closes.
    let (client_result, server_result) = tokio::join!(client_to_server, server_to_client);
    drop(tx);

    if let Err(error) = client_result {
        warn!(error = %error, "Client-to-server relay stopped with an error");
    }
    if let Err(error) = server_result {
        warn!(error = %error, "Server-to-client relay stopped with an error");
    }
    saver.await.context("capture saver task panicked")?;
    Ok(())
}

async fn save_events(mut rx: mpsc::Receiver<PacketEvent>) {
    let mut current_group: Option<RequestGroup> = None;

    while let Some(event) = rx.recv().await {
        match event {
            PacketEvent::Client(packet) => {
                if is_ping_packet(packet.cmd) {
                    continue;
                }
                if let Some(group) = current_group.take() {
                    if let Err(error) = save_request_group(&group).await {
                        warn!(error = %error, "Could not save packet request group");
                    }
                }
                current_group = Some(RequestGroup {
                    client_request: packet,
                    server_responses: Vec::new(),
                });
            }
            PacketEvent::Server(packet) => {
                if is_ping_packet(packet.cmd) {
                    continue;
                }
                if let Some(group) = current_group.as_mut() {
                    group.server_responses.push(packet);
                }
            }
        }
    }

    if let Some(group) = current_group {
        if let Err(error) = save_request_group(&group).await {
            warn!(error = %error, "Could not save final packet request group");
        }
    }
}

fn make_packet_info(direction: &str, cmd: u32, data: &[u8]) -> PacketInfo {
    let name = MsgId::try_from(cmd)
        .map(|id| id.to_string())
        .unwrap_or_else(|_| format!("UNKNOWN({cmd})"));
    let mut decoded = dispatch_cmd(cmd, data);
    if let Some(value) = decoded.as_mut() {
        redact_capture_value(cmd, value);
    }

    // Keep the raw bytes of server messages that have no schema yet so the
    // catalogue can be completed from a later capture. Client requests are not
    // recorded this way because unknown fields may contain credentials.
    let payload_hex = if decoded.is_none() && direction == "S->C" {
        Some(hex_encode(data, MAX_UNDECODED_HEX_BYTES))
    } else {
        None
    };

    PacketInfo {
        timestamp: Utc::now(),
        direction: direction.to_owned(),
        cmd,
        name,
        payload_len: data.len(),
        decoded,
        payload_hex,
    }
}

fn log_packet(packet: &PacketInfo) {
    info!(
        direction = %packet.direction,
        cmd = packet.cmd,
        name = %packet.name,
        payload_len = packet.payload_len,
        "Observed protocol packet"
    );
    if let Some(decoded) = &packet.decoded {
        tracing::debug!(direction = %packet.direction, %decoded, "Decoded packet");
    }
}
