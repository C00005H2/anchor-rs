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
use crate::sequence::TemplateFile;
use crate::messages::{
    CS_GUIDE_END, CS_MONTH_CARD_PANEL, CS_NORMAL_LOG, CS_RESET_HERO_LV_PRE_VIEW,
    SC_MONTH_CARD_PANEL,
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

/// Handle CS_GUIDE_END (12059): replay the recorded guide-state update when
/// available, otherwise treat it as a silent no-op.
pub async fn handle_guide_end(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_GUIDE_END,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    let Ok(script) = TemplateFile::load("world/guide_end.json") else {
        return Ok(Vec::new());
    };
    let Some(group) = script.first_group().cloned() else {
        return Ok(Vec::new());
    };
    let cursor = ctx.lock().await.replay_cursor.clone();
    group.encode(&cursor)
}

/// Handle CS_NORMAL_LOG (12068): client telemetry, no reply.
pub async fn handle_normal_log(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_NORMAL_LOG,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    Ok(Vec::new())
}

/// Handle CS_RESET_HERO_LV_PRE_VIEW (13364): preview-only request, no reply.
pub async fn handle_reset_hero_lv_preview(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_RESET_HERO_LV_PRE_VIEW,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    Ok(Vec::new())
}

/// Handle CS_MONTH_CARD_PANEL (24094): resend the month card panel snapshot.
pub async fn handle_month_card_panel(
    _ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_MONTH_CARD_PANEL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    Ok(vec![GameDataLoader::build_packet::<SC_MONTH_CARD_PANEL>(
        "hero_biography/month_card_panel.json",
        24095,
    )?])
}
