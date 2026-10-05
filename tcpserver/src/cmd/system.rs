use std::sync::Arc;
use tokio::sync::Mutex;
use crate::messages::{CS_SYS_PING, SC_SYS_PING};
use crate::state::ConnectionContext;
use crate::packet::build_server_packet;

/// Handle CS_SYS_PING (10000) -> SC_SYS_PING (10001)
pub async fn handle_ping(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_SYS_PING,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    {
        let mut connection = ctx.lock().await;
        connection.update_heartbeat();
    }

    let response = SC_SYS_PING {
        time: chrono::Utc::now().timestamp() as i32,
    };

    Ok(vec![build_server_packet(10001, &response.encode())?])
}

use crate::data_loader::GameDataLoader;
use crate::messages::{
    CS_PUBLIC_CHAT_SETTING, CS_REQ_MODULE_READ, SC_PUBLIC_CHAT_SETTING, SC_RES_MODULE_READ,
};
use serde::Deserialize;

const CHAT_DATA: &str = "chat/public_chat.json";

/// Chat channels extracted from a capture.
#[derive(Clone, Debug, Default, Deserialize)]
struct ChatTable {
    #[serde(default)]
    channels: Vec<SC_PUBLIC_CHAT_SETTING>,
}

/// Handle CS_PUBLIC_CHAT_SETTING (10054) -> SC_PUBLIC_CHAT_SETTING.
///
/// Joins/leaves the requested channel; unknown channels answer with an empty
/// room list instead of an error.
pub async fn handle_public_chat_setting(
    _ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_PUBLIC_CHAT_SETTING,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let table: ChatTable = GameDataLoader::load_struct(CHAT_DATA)?;
    let response = table
        .channels
        .into_iter()
        .find(|channel| channel.channel == request.channel)
        .unwrap_or(SC_PUBLIC_CHAT_SETTING {
            channel: request.channel,
            room_now: 0,
            people_count: 0,
            room_list: Vec::new(),
        });
    tracing::info!(channel = response.channel, "Public chat channel selected");
    Ok(vec![build_server_packet(10055, &response.encode())?])
}

/// Handle CS_REQ_MODULE_READ (10057) -> SC_RES_MODULE_READ.
///
/// The server acknowledges which module entry the client has read.
pub async fn handle_req_module_read(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_REQ_MODULE_READ,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let claim_id = (i64::from(request.msg_type) << 32) | (i64::from(request.id) & 0xffffffff);
    ctx.lock().await.claim_once("module_read", claim_id);
    tracing::debug!(msg_type = request.msg_type, id = request.id, "Module read acknowledged");
    Ok(vec![build_server_packet(
        10058,
        &SC_RES_MODULE_READ {
            msg_type: request.msg_type,
            id: request.id,
        }
        .encode(),
    )?])
}
