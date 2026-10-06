//! Battle flow driven by a recorded battle script.
//!
//! The capture contains one complete battle: field entry, auto mode, and 13
//! `CS_BATTLE_VIDEO_END` action batches ending in the battle result.  Those
//! batches are stored under `battle/` and consumed in order per connection, so
//! every client walks the same scripted fight.  Attribute updates inside the
//! script are absorbed into the local profile so later claims stay coherent.

use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use crate::{
    messages::{CS_BATTLE_AUTO, CS_BATTLE_FIELD_ENTER, CS_BATTLE_START, CS_BATTLE_VIDEO_END},
    sequence::{TemplateFile, TemplateGroup},
    state::ConnectionContext,
};

const ENTER_DATA: &str = "battle/enter.json";
const AUTO_DATA: &str = "battle/auto.json";
const VIDEO_END_DATA: &str = "battle/video_end.json";

/// Remember the absolute attribute values a scripted response reports.
pub(crate) fn absorb_attr_updates(connection: &mut ConnectionContext, group: &TemplateGroup) {
    for response in &group.responses {
        if response.cmd != 12003 {
            continue;
        }
        let Some(attrs) = response
            .decoded
            .as_ref()
            .and_then(|decoded| decoded.get("attr_list"))
            .and_then(|list| list.as_array())
        else {
            continue;
        };
        for attr in attrs {
            let key = attr.get("key").and_then(|key| key.as_i64());
            let value = attr.get("value").and_then(|value| value.as_str());
            if let (Some(key), Some(value)) = (key, value) {
                connection.record_attr(key as i16, value.to_owned());
            }
        }
    }
}

/// Handle CS_BATTLE_FIELD_ENTER (20100).
///
/// Replies with the recorded entry sequence.  The `SC_CHANGE_HERO` payload is
/// patched with the formation the client reported via `CS_CHANGE_HERO`, when
/// it sent one.
pub async fn handle_battle_field_enter(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_FIELD_ENTER,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(ENTER_DATA)?;
    let Some(mut group) = script.first_group().cloned() else {
        return Ok(Vec::new());
    };

    let (cursor, formation) = {
        let mut connection = ctx.lock().await;
        connection.battle_active = true;
        connection.battle_script_index = 0;
        (connection.replay_cursor.clone(), connection.formation.clone())
    };

    if !formation.is_empty() {
        for response in group.responses.iter_mut() {
            if response.cmd == 13047 {
                if let Some(object) = response.decoded.as_mut().and_then(|decoded| decoded.as_object_mut()) {
                    object.insert(
                        "formation_list".to_owned(),
                        serde_json::to_value(&formation)?,
                    );
                }
            }
        }
    }

    info!(
        battle_type = request.battle_type,
        battle_field_id = %request.battle_field_id,
        "Battle field entered"
    );
    group.encode(&cursor)
}

/// Handle CS_BATTLE_START (20102): the real server sent no reply.
pub async fn handle_battle_start(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_BATTLE_START,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut connection = ctx.lock().await;
    connection.battle_active = true;
    info!("Battle started");
    Ok(Vec::new())
}

/// Handle CS_BATTLE_AUTO (20113): acknowledge and push the first actions.
pub async fn handle_battle_auto(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_AUTO,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(AUTO_DATA)?;
    let Some(mut group) = script.first_group().cloned() else {
        return Ok(Vec::new());
    };

    // Echo the requested auto-battle flag in SC_BATTLE_AUTO.
    for response in group.responses.iter_mut() {
        if response.cmd == 20114 {
            if let Some(object) = response.decoded.as_mut().and_then(|decoded| decoded.as_object_mut()) {
                object.insert("is_auto".to_owned(), serde_json::json!(request.is_auto));
            }
        }
    }

    let cursor = ctx.lock().await.replay_cursor.clone();
    info!(is_auto = request.is_auto, "Auto-battle requested");
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, &group);
    }
    Ok(packets)
}

/// Handle CS_BATTLE_VIDEO_END (20104): feed the next scripted action batch.
///
/// Batches are consumed in capture order; when the recording is exhausted the
/// battle ends silently, like the capture's final `sync_word = 0` request.
pub async fn handle_battle_video_end(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_VIDEO_END,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    if request.sync_word == 0 {
        let mut connection = ctx.lock().await;
        connection.battle_active = false;
        connection.battle_script_index = 0;
        info!("Battle finished");
        return Ok(Vec::new());
    }

    let script = TemplateFile::load(VIDEO_END_DATA)?;
    let (cursor, group) = {
        let mut connection = ctx.lock().await;
        let index = connection.battle_script_index;
        if index >= script.group_count() {
            connection.battle_active = false;
            return Ok(Vec::new());
        }
        connection.battle_script_index = index + 1;
        let group = script.group(index).cloned().unwrap_or_default();
        (connection.replay_cursor.clone(), group)
    };

    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, &group);
    }
    Ok(packets)
}
