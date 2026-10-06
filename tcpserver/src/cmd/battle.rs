//! Battle flow driven by recorded battle sessions.
//!
//! Each `CS_BATTLE_FIELD_ENTER` in the capture starts a *session* stored under
//! `battle/session_{n}.json` with the entry sequence, the auto-battle push and
//! every action batch (`CS_BATTLE_VIDEO_END`, `CS_BATTLE_USE_SKILL`,
//! `CS_BATTLE_SYNC`) in capture order.  The final batch of a session carries
//! `SC_BATTLE_RESULT` plus the captured reward updates (XP, items, level-ups),
//! so completed battles replay their rewards byte-exact. Explicit quit/skip
//! requests instead receive a non-rewarding retreat result. Attribute updates
//! inside the script are absorbed into the local profile for later claims.
//!
//! When no session files exist the legacy flat `battle/video_end.json` queue
//! is used instead.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::info;

use crate::{
    capture_replay::decode_payload_hex,
    data_loader::GameDataLoader,
    messages::{
        CS_BATTLE_AUTO, CS_BATTLE_FIELD_ENTER, CS_BATTLE_START, CS_BATTLE_SYNC,
        CS_BATTLE_USE_SKILL, CS_BATTLE_VIDEO_END, SC_BATTLE_NONE, SC_BATTLE_RESULT,
        SC_BATTLE_USE_SKILL,
    },
    packet::build_server_packet,
    sequence::{TemplateFile, TemplateGroup, TemplateResponse},
    state::{BattlePendingSkill, ConnectionContext},
};

const ENTER_DATA: &str = "battle/enter.json";
const AUTO_DATA: &str = "battle/auto.json";
const VIDEO_END_DATA: &str = "battle/video_end.json";
const SESSION_PREFIX: &str = "battle/session_";
const MAX_SESSIONS: usize = 32;

/// One recorded action batch inside a battle session.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct BattleStep {
    /// The CS command that produced this batch (20104/20108/20120).
    #[serde(default)]
    pub request_cmd: u32,
    #[serde(default)]
    pub responses: Vec<TemplateResponse>,
}

/// A complete recorded battle: entry, auto push and ordered action batches.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct BattleSessionData {
    #[serde(default)]
    pub field_id: Option<String>,
    #[serde(default)]
    pub battle_type: Option<i8>,
    #[serde(default)]
    pub enter: Option<TemplateGroup>,
    #[serde(default)]
    pub auto: Option<TemplateGroup>,
    #[serde(default)]
    pub steps: Vec<BattleStep>,
}

impl BattleStep {
    fn as_group(&self) -> TemplateGroup {
        TemplateGroup {
            responses: self.responses.clone(),
        }
    }

    fn contains(&self, cmd: u32) -> bool {
        self.responses.iter().any(|response| response.cmd == cmd)
    }
}

fn load_session(index: usize) -> Option<BattleSessionData> {
    GameDataLoader::load_struct::<BattleSessionData>(&format!("{SESSION_PREFIX}{}.json", index + 1))
        .ok()
}

fn session_count() -> usize {
    let mut count = 0;
    while count < MAX_SESSIONS && load_session(count).is_some() {
        count += 1;
    }
    count
}

/// Remember the absolute attribute values a scripted response reports, and
/// track the newest battle sync word so out-of-script skill requests can be
/// acknowledged with a plausible sync value.
pub(crate) fn absorb_attr_updates(connection: &mut ConnectionContext, group: &TemplateGroup) {
    for response in &group.responses {
        if matches!(
            response.cmd,
            20101 | 20103 | 20105 | 20106 | 20114 | 20115 | 20118 | 20125 | 20126 | 20129
        ) {
            if let Some(sync) = response
                .decoded
                .as_ref()
                .and_then(|decoded| decoded.get("sync_word"))
                .and_then(|sync| sync.as_i64())
            {
                connection.battle_sync_word = sync as i32;
            }
        }
        if matches!(response.cmd, 20105 | 20106) {
            if let Some(round) = response
                .decoded
                .as_ref()
                .and_then(|decoded| decoded.get("round"))
                .and_then(Value::as_i64)
            {
                connection.battle_round = round as i8;
            }
        }
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

fn mapped_actor(
    actor_map: &HashMap<i32, (i32, i32, i32)>,
    actor_id: i32,
) -> Option<(i32, i32, i32)> {
    actor_map
        .get(&actor_id)
        .copied()
        .or_else(|| actor_map.values().min_by_key(|mapped| mapped.0).copied())
}

fn remap_skill_id(skill_id: i32, old_tid: i32, new_tid: i32) -> i32 {
    if skill_id >= 0 && skill_id / 100 == old_tid {
        new_tid.saturating_mul(100).saturating_add(skill_id % 100)
    } else {
        skill_id
    }
}

fn remap_side_heroes(value: &mut Value, actor_map: &HashMap<i32, (i32, i32, i32)>) {
    match value {
        Value::Object(object) => {
            let side = object.get("side").and_then(Value::as_i64);
            let old_id = object.get("hero_id").and_then(Value::as_i64);
            if side == Some(1) {
                if let Some(old_id) = old_id {
                    if let Some((new_id, _, _)) = mapped_actor(actor_map, old_id as i32) {
                        object.insert("hero_id".to_owned(), json!(new_id));
                    }
                }
            }
            for child in object.values_mut() {
                remap_side_heroes(child, actor_map);
            }
        }
        Value::Array(items) => {
            items.retain_mut(|item| {
                if let Some(object) = item.as_object_mut() {
                    if object.get("side").and_then(Value::as_i64) == Some(1) {
                        if let Some(old_id) = object.get("hero_id").and_then(Value::as_i64) {
                            let Some((new_id, _, _)) = mapped_actor(actor_map, old_id as i32) else {
                                return false;
                            };
                            object.insert("hero_id".to_owned(), json!(new_id));
                        }
                    }
                }
                remap_side_heroes(item, actor_map);
                true
            });
        }
        _ => {}
    }
}

fn remap_order_list(value: &mut Value, actor_map: &HashMap<i32, (i32, i32, i32)>) {
    let Some(items) = value.as_array_mut() else { return };
    items.retain_mut(|item| {
        let Some(object) = item.as_object_mut() else { return true };
        if object.get("key").and_then(Value::as_i64) != Some(1) {
            return true;
        }
        let Some(old_id) = object.get("value").and_then(Value::as_i64) else {
            return false;
        };
        let Some((new_id, _, _)) = mapped_actor(actor_map, old_id as i32) else {
            return false;
        };
        object.insert("value".to_owned(), json!(new_id));
        true
    });
}

/// Remap every player-side actor reference before a captured group is sent.
/// Raw field-info payloads are handled separately because their hero record has
/// a trailing field not represented by the generated message struct.
fn remap_battle_response(
    response: &mut TemplateResponse,
    actor_map: &HashMap<i32, (i32, i32, i32)>,
) {
    if let Some(decoded) = response.decoded.as_mut() {
        match response.cmd {
            20103 => {
                if let Some(object) = decoded.as_object_mut() {
                    if object.get("side").and_then(Value::as_i64) == Some(1) {
                        if let Some(old_id) = object.get("hero_id").and_then(Value::as_i64) {
                            if let Some((new_id, old_tid, new_tid)) =
                                mapped_actor(actor_map, old_id as i32)
                            {
                                object.insert("hero_id".to_owned(), json!(new_id));
                                if let Some(skill_id) =
                                    object.get("skill_id").and_then(Value::as_i64)
                                {
                                    object.insert(
                                        "skill_id".to_owned(),
                                        json!(remap_skill_id(skill_id as i32, old_tid, new_tid)),
                                    );
                                }
                            }
                        }
                    }
                    for key in ["target_list", "hero_list", "result_list"] {
                        if let Some(nested) = object.get_mut(key) {
                            remap_side_heroes(nested, actor_map);
                        }
                    }
                    if object.get("target_side").and_then(Value::as_i64) == Some(1) {
                        if let Some(old_id) = object.get("target_id").and_then(Value::as_i64) {
                            if let Some((new_id, _, _)) = mapped_actor(actor_map, old_id as i32) {
                                object.insert("target_id".to_owned(), json!(new_id));
                            }
                        }
                    }
                }
            }
            20105 | 20126 => {
                if let Some(object) = decoded.as_object_mut() {
                    for key in ["curl_order", "next_order"] {
                        if let Some(order) = object.get_mut(key) {
                            remap_order_list(order, actor_map);
                        }
                    }
                }
            }
            20118 => {
                if let Some(object) = decoded.as_object_mut() {
                    if object.get("side").and_then(Value::as_i64) == Some(1) {
                        // Story/trigger hero changes must not reintroduce an
                        // attacker that was removed from the ready formation.
                        object.insert("hero_list".to_owned(), json!([]));
                    }
                }
            }
            20129 => {
                if let Some(object) = decoded.as_object_mut() {
                    if object.get("hero_side").and_then(Value::as_i64) == Some(1) {
                        if let Some(old_id) = object.get("hero_id").and_then(Value::as_i64) {
                            if let Some((new_id, _, new_tid)) =
                                mapped_actor(actor_map, old_id as i32)
                            {
                                object.insert("hero_id".to_owned(), json!(new_id));
                                object.insert("hero_tid".to_owned(), json!(new_tid));
                            }
                        }
                    }
                }
            }
            20106 => {
                if let Some(object) = decoded.as_object_mut() {
                    if let Some(items) = object.get_mut("statistic").and_then(Value::as_array_mut) {
                        items.retain_mut(|item| {
                            let Some(stat) = item.as_object_mut() else { return true };
                            if stat.get("side").and_then(Value::as_i64) != Some(1) {
                                return true;
                            }
                            let Some(old_id) = stat.get("hero_id").and_then(Value::as_i64) else {
                                return false;
                            };
                            let Some((new_id, _, new_tid)) =
                                mapped_actor(actor_map, old_id as i32)
                            else {
                                return false;
                            };
                            stat.insert("hero_id".to_owned(), json!(new_id));
                            stat.insert("tid".to_owned(), json!(new_tid));
                            true
                        });
                    }
                    if let Some(ids) = object.get_mut("hero_id_list").and_then(Value::as_array_mut) {
                        ids.retain_mut(|entry| {
                            let Some(item) = entry.as_object_mut() else { return true };
                            let Some(old_id) = item.get("key").and_then(Value::as_i64) else {
                                return true;
                            };
                            let Some((new_id, _, _)) = mapped_actor(actor_map, old_id as i32) else {
                                return false;
                            };
                            item.insert("key".to_owned(), json!(new_id));
                            true
                        });
                    }
                }
            }
            20125 => remap_side_heroes(decoded, actor_map),
            _ => {}
        }
        if matches!(response.cmd, 20103 | 20105 | 20106 | 20118 | 20125 | 20126 | 20129) {
            response.payload_hex = None;
        }
        return;
    }

    // If a legacy capture has raw-only action packets, at least rewrite their
    // acting hero and skill without risking a lossy full-message re-encode.
    let Some(mut raw) = response.payload_hex.as_deref().and_then(decode_payload_hex) else {
        return;
    };
    match response.cmd {
        20103 if raw.len() >= 9 && raw[0] == 1 => {
            let Some(old_id) = be_i32(&raw, 1) else { return };
            let Some((new_id, old_tid, new_tid)) = mapped_actor(actor_map, old_id) else { return };
            raw[1..5].copy_from_slice(&new_id.to_be_bytes());
            if let Some(skill_id) = be_i32(&raw, 5) {
                raw[5..9].copy_from_slice(&remap_skill_id(skill_id, old_tid, new_tid).to_be_bytes());
            }
        }
        _ => return,
    }
    response.payload_hex = Some(to_hex(&raw));
}

/// Clone the first player action for newly added deployed heroes. This keeps
/// heroes absent from the recording from standing idle in the replay.
fn injected_actions(added: &[(i32, i32)], responses: &[TemplateResponse]) -> Vec<Vec<u8>> {
    let Some(template) = responses.iter().find(|response| response.cmd == 20103) else {
        return Vec::new();
    };
    let Some(decoded) = template.decoded.as_ref() else { return Vec::new() };
    if decoded.get("side").and_then(Value::as_i64) != Some(1) {
        return Vec::new();
    }
    added
        .iter()
        .filter_map(|(hero_id, tid)| {
            let mut decoded = decoded.clone();
            let object = decoded.as_object_mut()?;
            object.insert("hero_id".to_owned(), json!(hero_id));
            object.insert("skill_id".to_owned(), json!(tid.saturating_mul(100).saturating_add(1)));
            crate::capture_replay::encode_captured_response(&crate::capture_replay::CapturedResponse {
                cmd: 20103,
                decoded,
                raw: None,
            })
            .ok()
            .flatten()
        })
        .collect()
}

/// Remap, encode and account for one server-response group.
async fn encode_group(
    ctx: &Arc<Mutex<ConnectionContext>>,
    mut group: TemplateGroup,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let (cursor, added, actor_map, pending_skill, suppress_attacker) = {
        let connection = ctx.lock().await;
        (
            connection.replay_cursor.clone(),
            connection.battle_added_heroes.clone(),
            connection.battle_actor_map.clone(),
            connection.battle_pending_skills.front().copied(),
            connection.formation_received && connection.battle_active_heroes.is_empty(),
        )
    };
    let mut applied_pending_skill = false;
    for response in &mut group.responses {
        if !actor_map.is_empty() || suppress_attacker {
            remap_battle_response(response, &actor_map);
        }
        if response.cmd != 20103 {
            continue;
        }
        if let Some(skill) = pending_skill {
            if let Some(decoded) = response.decoded.as_mut() {
                if decoded.get("side").and_then(Value::as_i64) == Some(1) {
                    if let Some(object) = decoded.as_object_mut() {
                        object.insert("hero_id".to_owned(), json!(skill.hero_id));
                        object.insert("skill_id".to_owned(), json!(skill.skill_id));
                        response.payload_hex = None;
                        applied_pending_skill = true;
                    }
                }
            } else if let Some(mut raw) =
                response.payload_hex.as_deref().and_then(decode_payload_hex)
            {
                if raw.len() >= 9 && raw[0] == 1 {
                    raw[1..5].copy_from_slice(&skill.hero_id.to_be_bytes());
                    raw[5..9].copy_from_slice(&skill.skill_id.to_be_bytes());
                    response.payload_hex = Some(to_hex(&raw));
                    applied_pending_skill = true;
                }
            }
        }
    }
    if suppress_attacker {
        group.responses.retain(|response| match response.cmd {
            20103 | 20129 => {
                let field = if response.cmd == 20103 { "side" } else { "hero_side" };
                let side = response
                    .decoded
                    .as_ref()
                    .and_then(|decoded| decoded.get(field))
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        response
                            .payload_hex
                            .as_deref()
                            .and_then(decode_payload_hex)
                            .and_then(|raw| raw.first().copied())
                            .map(i64::from)
                    });
                side != Some(1)
            }
            20115 => false,
            _ => true,
        });
    }
    let mut packets = group.encode(&cursor)?;
    let extra_actions = injected_actions(&added, &group.responses);
    let mut insert_at = packets
        .iter()
        .position(|packet| {
            packet.len() >= 6
                && u32::from_be_bytes([packet[2], packet[3], packet[4], packet[5]]) == 20103
        })
        .map(|index| index + 1)
        .unwrap_or(packets.len());
    for packet in extra_actions {
        packets.insert(insert_at, packet);
        insert_at += 1;
    }
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, &group);
        if applied_pending_skill {
            connection.battle_pending_skills.pop_front();
        }
        if group.responses.iter().any(|response| response.cmd == 20106) {
            connection.battle_result_served = true;
            connection.battle_active = false;
        }
    }
    Ok(packets)
}

async fn encode_step(
    ctx: &Arc<Mutex<ConnectionContext>>,
    step: &BattleStep,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    encode_group(ctx, step.as_group()).await
}

/// Pick the next unconsumed step the requester may consume.
///
/// Recorded steps form a single timeline: an AUTO recording is all 20104
/// action batches, a MANUAL recording interleaves 20104 action batches with
/// 20108 skill batches, and either may contain 20120 sync steps.  To let an
/// auto client advance through a manual recording (and a manual client tap
/// through an auto one), each request consumes in timeline order:
///   * 20104 takes the next action OR skill step (never sync steps — those
///     are reserved for the client's own 20120 requests),
///   * 20108 takes the next skill step only (the caller synthesises an ack
///     when none are left),
///   * 20120 takes the next sync step only.
async fn take_step(
    ctx: &Arc<Mutex<ConnectionContext>>,
    request_cmd: u32,
) -> Option<BattleStep> {
    let consumes = |step_cmd: u32| match request_cmd {
        20104 => step_cmd == 20104 || step_cmd == 20108,
        other => step_cmd == other,
    };
    let mut connection = ctx.lock().await;
    if !connection.battle_active || connection.battle_result_served {
        return None;
    }
    let chosen = connection.battle_session_chosen?;
    let session = load_session(chosen)?;
    for (index, step) in session.steps.iter().enumerate() {
        let consumed = connection.battle_step_consumed.get(index).copied().unwrap_or(true);
        if !consumed && consumes(step.request_cmd) {
            if let Some(flag) = connection.battle_step_consumed.get_mut(index) {
                *flag = true;
            }
            return Some(step.clone());
        }
    }
    None
}

/// Handle CS_BATTLE_FIELD_ENTER (20100).
///
/// Picks the recorded session matching the requested battlefield (scanning
/// forward from the last played session), falling back to the next session in
/// capture order.  The `SC_CHANGE_HERO` payload is patched with the formation
/// the client reported via `CS_CHANGE_HERO`, when it sent one.
pub async fn handle_battle_field_enter(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_FIELD_ENTER,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let count = session_count();
    if count == 0 {
        // Legacy single-battle data (pre-session captures).
        return handle_legacy_field_enter(ctx, request).await;
    }

    let (formation, formation_received, ready_team_id, mut group) = {
        let mut connection = ctx.lock().await;
        connection.clear_battle_runtime();
        let start = connection.battle_session_index.min(count - 1);
        let mut chosen = None;
        let mut fallback = None;
        for offset in 0..count {
            let index = (start + offset) % count;
            let Some(session) = load_session(index) else { continue };
            let matches = session
                .field_id
                .as_deref()
                .map(|field_id| field_id == request.battle_field_id)
                .unwrap_or(false)
                && session
                    .battle_type
                    .map(|battle_type| battle_type == request.battle_type)
                    .unwrap_or(true);
            if matches {
                chosen = Some((index, session));
                break;
            }
            if fallback.is_none() {
                fallback = Some((index, session));
            }
        }
        let (index, session) = chosen.or(fallback).unwrap_or_else(|| {
            let session = load_session(start).unwrap_or_default();
            (start, session)
        });
        connection.battle_session_index = (index + 1) % count;
        connection.battle_session_chosen = Some(index);
        connection.battle_step_consumed = vec![false; session.steps.len()];
        connection.battle_auto_served = false;
        connection.battle_active = true;
        connection.battle_result_served = false;
        connection.battle_script_index = 0;
        connection.battle_added_heroes = Vec::new();
        info!(
            battle_type = request.battle_type,
            battle_field_id = %request.battle_field_id,
            session = index + 1,
            steps = session.steps.len(),
            "Battle field entered (session)"
        );
        let group = session.enter.clone().unwrap_or_default();
        (
            connection.formation.clone(),
            connection.formation_received,
            connection.ready_team_id,
            group,
        )
    };

    if formation_received {
        for response in group.responses.iter_mut() {
            if response.cmd == 13047 {
                if let Some(object) = response.decoded.as_mut().and_then(|decoded| decoded.as_object_mut()) {
                    object.insert(
                        "formation_list".to_owned(),
                        serde_json::to_value(&formation)?,
                    );
                    // The patched formation must win over the recorded raw
                    // bytes, otherwise freshly recruited heroes never show up
                    // on the deploy screen.
                    response.payload_hex = None;
                }
            }
        }
        let deployed = deployed_heroes(&formation, ready_team_id);
        let roster = patch_enter_formation(&mut group, &deployed);
        let mut connection = ctx.lock().await;
        connection.battle_active_heroes = deployed;
        connection.battle_actor_map = roster.actor_map;
        connection.battle_added_heroes = roster.added_heroes;
    }

    encode_group(&ctx, group).await
}

/// Legacy entry path for captures without per-battle session files.
async fn handle_legacy_field_enter(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_FIELD_ENTER,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(ENTER_DATA)?;
    let Some(mut group) = script.first_group().cloned() else {
        return Ok(Vec::new());
    };

    let (formation, formation_received, ready_team_id) = {
        let mut connection = ctx.lock().await;
        connection.clear_battle_runtime();
        connection.battle_active = true;
        connection.battle_result_served = false;
        connection.battle_script_index = 0;
        connection.battle_session_chosen = None;
        connection.battle_auto_served = false;
        (
            connection.formation.clone(),
            connection.formation_received,
            connection.ready_team_id,
        )
    };

    if formation_received {
        for response in group.responses.iter_mut() {
            if response.cmd == 13047 {
                if let Some(object) = response.decoded.as_mut().and_then(|decoded| decoded.as_object_mut()) {
                    object.insert(
                        "formation_list".to_owned(),
                        serde_json::to_value(&formation)?,
                    );
                    // The patched formation must win over the recorded raw
                    // bytes, otherwise freshly recruited heroes never show up
                    // on the deploy screen.
                    response.payload_hex = None;
                }
            }
        }
        let deployed = deployed_heroes(&formation, ready_team_id);
        let roster = patch_enter_formation(&mut group, &deployed);
        let mut connection = ctx.lock().await;
        connection.battle_active_heroes = deployed;
        connection.battle_actor_map = roster.actor_map;
        connection.battle_added_heroes = roster.added_heroes;
    }

    info!(
        battle_type = request.battle_type,
        battle_field_id = %request.battle_field_id,
        "Battle field entered"
    );
    encode_group(&ctx, group).await
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
    if !ctx.lock().await.battle_active {
        return Ok(Vec::new());
    }
    let mut group = {
        let connection = ctx.lock().await;
        let session_group = connection
            .battle_session_chosen
            .and_then(load_session)
            .and_then(|session| session.auto);
        session_group
    }
    .or_else(|| TemplateFile::load(AUTO_DATA).ok().and_then(|file| file.first_group().cloned()));

    let Some(group) = group.as_mut() else {
        return Ok(Vec::new());
    };

    // Only the first auto request of a battle gets the full opening push;
    // later toggles (e.g. switching to manual mid-fight) are acknowledged
    // with SC_BATTLE_AUTO alone, like the capture shows.
    let repeat = {
        let mut connection = ctx.lock().await;
        let repeat = connection.battle_auto_served;
        connection.battle_auto_served = true;
        repeat
    };
    if repeat {
        group.responses.retain(|response| response.cmd == 20114);
    }

    // Echo the requested auto-battle flag in SC_BATTLE_AUTO.
    for response in group.responses.iter_mut() {
        if response.cmd == 20114 {
            if let Some(object) = response.decoded.as_mut().and_then(|decoded| decoded.as_object_mut()) {
                object.insert("is_auto".to_owned(), serde_json::json!(request.is_auto));
                response.payload_hex = None;
            }
        }
    }

    info!(is_auto = request.is_auto, "Auto-battle requested");
    encode_group(&ctx, group.clone()).await
}

/// Handle CS_BATTLE_VIDEO_END (20104): feed the next scripted action batch.
///
/// Batches are consumed in capture order.  When the recording for the current
/// battle is exhausted the result batch (if any) is re-served so rewards, XP
/// and level-ups always reach the client.
pub async fn handle_battle_video_end(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_VIDEO_END,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    if request.sync_word == 0 {
        info!("Battle video ended without a sync word");
        return serve_battle_result(ctx, "video-end").await;
    }
    if !ctx.lock().await.battle_active {
        return Ok(Vec::new());
    }

    if let Some(step) = take_step(&ctx, 20104).await {
        return encode_step(&ctx, &step).await;
    }

    // Out of recorded video-end batches: re-serve the result batch so the
    // client still receives rewards when step counts diverge.
    let (result_served, reward) = {
        let connection = ctx.lock().await;
        (
            connection.battle_result_served,
            connection
                .battle_session_chosen
                .and_then(load_session)
                .and_then(|session| {
                    session.steps.iter().rev().find(|step| step.contains(20106)).cloned()
                }),
        )
    };
    if result_served {
        return Ok(Vec::new());
    }
    if let Some(reward) = reward {
        return encode_step(&ctx, &reward).await;
    }

    // Legacy flat queue (pre-session captures).
    handle_legacy_video_end(ctx).await
}

/// Legacy video-end consumption for captures without session files.
async fn handle_legacy_video_end(
    ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(VIDEO_END_DATA)?;
    let group = {
        let mut connection = ctx.lock().await;
        if connection.battle_session_chosen.is_some() {
            return Ok(Vec::new());
        }
        let index = connection.battle_script_index;
        if index >= script.group_count() {
            connection.battle_active = false;
            return Ok(Vec::new());
        }
        connection.battle_script_index = index + 1;
        script.group(index).cloned().unwrap_or_default()
    };

    encode_group(&ctx, group).await
}

/// Handle CS_BATTLE_USE_SKILL (20108): replay the recorded skill batch.
///
/// Manual battles interleave skill batches with action batches; each recorded
/// batch is consumed once, in capture order.  Outside a recorded manual
/// battle the real server replied with nothing.
pub async fn handle_battle_use_skill(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_USE_SKILL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    if !ctx.lock().await.battle_active {
        return Ok(Vec::new());
    }
    let (hero_id, tid, sync_word) = {
        let mut connection = ctx.lock().await;
        let selected = connection
            .battle_active_heroes
            .iter()
            .find(|(_, hero_tid, _)| request.skill_id / 100 == *hero_tid)
            .or_else(|| connection.battle_active_heroes.first());
        let (hero_id, tid) = selected
            .map(|(hero_id, tid, _)| (*hero_id, *tid))
            .unwrap_or_default();
        if hero_id != 0 {
            connection.battle_pending_skills.push_back(BattlePendingSkill {
                hero_id,
                skill_id: request.skill_id,
            });
        }
        (hero_id, tid, connection.battle_sync_word)
    };

    if let Some(mut step) = take_step(&ctx, 20108).await {
        for response in &mut step.responses {
            if response.cmd != 20115 {
                continue;
            }
            if let Some(object) = response.decoded.as_mut().and_then(Value::as_object_mut) {
                object.insert("hero_id".to_owned(), json!(hero_id));
                object.insert("skill_id".to_owned(), json!(request.skill_id));
                response.payload_hex = None;
            } else if let Some(mut raw) = response.payload_hex.as_deref().and_then(decode_payload_hex) {
                if raw.len() >= 8 {
                    raw[..4].copy_from_slice(&hero_id.to_be_bytes());
                    raw[4..8].copy_from_slice(&request.skill_id.to_be_bytes());
                    response.payload_hex = Some(to_hex(&raw));
                }
            }
        }
        return encode_step(&ctx, &step).await;
    }

    // A manual client needs an acknowledgement even when its battle was not
    // captured in manual mode. Queue the chosen skill for the next player
    // action batch so the scripted action actually uses it.
    info!(
        hero_id,
        tid,
        skill_id = request.skill_id,
        sync_word,
        "Battle skill acknowledged (no recorded batch)"
    );
    Ok(vec![build_server_packet(
        20115,
        &SC_BATTLE_USE_SKILL {
            hero_id,
            skill_id: request.skill_id,
            result: 1,
            skill_soul: 0,
            rage: 10000,
            sync_word,
        }
        .encode(),
    )?])
}

/// Handle CS_BATTLE_SYNC (20120): replay the recorded sync batch, if any.
pub async fn handle_battle_sync(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_BATTLE_SYNC,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    if let Some(step) = take_step(&ctx, 20120).await {
        return encode_step(&ctx, &step).await;
    }
    if ctx.lock().await.battle_active {
        let response = SC_BATTLE_NONE {};
        return Ok(vec![build_server_packet(20116, &response.encode())?]);
    }
    Ok(Vec::new())
}

/// Send a non-rewarding retreat result so quit/skip leaves the battle screen
/// without replaying the captured session's victory rewards.
fn retreat_result(round: i8) -> SC_BATTLE_RESULT {
    SC_BATTLE_RESULT {
        result: 3,
        award: Vec::new(),
        detail_item_award: Vec::new(),
        player_exp: 0,
        hero_exp: 0,
        hero_relation: 0,
        args: Vec::new(),
        hero_id_list: Vec::new(),
        round,
        statistic: Vec::new(),
        pos_effect: Vec::new(),
        is_replay: 0,
    }
}

async fn serve_battle_result(
    ctx: Arc<Mutex<ConnectionContext>>,
    kind: &str,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let round = {
        let mut connection = ctx.lock().await;
        if connection.battle_result_served {
            return Ok(Vec::new());
        }
        let round = connection.battle_round;
        connection.clear_battle_runtime();
        round
    };
    info!(kind, round, "Battle abandoned by client");
    let result = retreat_result(round);
    Ok(vec![build_server_packet(20106, &result.encode())?])
}

/// Handle CS_BATTLE_QUIT (20107): the client abandons the current battle.
pub async fn handle_battle_quit(
    ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    serve_battle_result(ctx, "quit").await
}

/// Handle CS_BATTLE_SKIP (20109): the client skips ahead / retreats.
pub async fn handle_battle_skip(
    ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    serve_battle_result(ctx, "skip").await
}

/// Heroes the client actually deployed, in formation order, as
/// (id, tid, formation slot).
///
/// Prefers the ready-marked team, then team 1001, then the first team with
/// any heroes.
fn deployed_heroes(
    formation: &[crate::messages::pt_hero_formation],
    ready_team_id: Option<i16>,
) -> Vec<(i32, i32, i8)> {
    let team = ready_team_id
        .and_then(|team_id| formation.iter().find(|team| team.team_id == team_id))
        .or_else(|| formation.iter().find(|team| team.is_ready == 1))
        .or_else(|| formation.iter().find(|team| team.team_id == 1001))
        .or_else(|| formation.iter().find(|team| !team.formation_hero_list.is_empty()));
    team.map(|team| {
        team.formation_hero_list
            .iter()
            .map(|hero| (hero.hero_id, hero.tid, hero.pos))
            .collect()
    })
    .unwrap_or_default()
}

/// Formation slot -> battle grid cell, as observed from real-server
/// `SC_BATTLE_FIELD_INFO` replies (slots 1-5); the rest are spare cells.
fn slot_cell(slot: i8, used: &[(i16, i16)]) -> (i16, i16) {
    let known = [
        (1i8, (2i16, 1i16)),
        (2, (1, 2)),
        (3, (4, 2)),
        (4, (3, 2)),
        (5, (2, 3)),
    ];
    if let Some((_, cell)) = known.iter().find(|(s, _)| *s == slot) {
        if !used.contains(cell) {
            return *cell;
        }
    }
    let spares = [
        (2i16, 1i16), (1, 2), (4, 2), (3, 2), (2, 3), (1, 1), (3, 1), (4, 1),
        (1, 3), (3, 3), (4, 3),
    ];
    spares
        .iter()
        .find(|cell| !used.contains(cell))
        .copied()
        .unwrap_or((1, 1))
}

/// Build one attacker hero entry the way the real server does for heroes it
/// has no recorded battle data for: base stats per tid, the generic skill
/// triple (tid*100+1, tid*100+4, tid) and neutral cosmetics.
fn synth_hero_entry(id: i32, tid: i32, cell: (i16, i16)) -> Vec<u8> {
    let (max_hp, color) = match tid {
        1110 => (618i64, 3i16),
        1305 => (686, 3),
        1206 => (541, 3),
        1006 => (529, 4),
        1304 => (427, 2),
        1205 => (500, 3),
        _ => (500, 3),
    };
    let mut buf: Vec<u8> = Vec::with_capacity(69);
    buf.extend_from_slice(&id.to_be_bytes());
    buf.extend_from_slice(&tid.to_be_bytes());
    buf.push(0); // msg_type
    buf.extend_from_slice(&cell.0.to_be_bytes());
    buf.extend_from_slice(&cell.1.to_be_bytes());
    buf.extend_from_slice(&max_hp.to_be_bytes());
    buf.extend_from_slice(&max_hp.to_be_bytes());
    buf.extend_from_slice(&0i32.to_be_bytes()); // rage
    buf.push(5); // skill_soul
    buf.extend_from_slice(&1i16.to_be_bytes()); // lv
    buf.extend_from_slice(&0i16.to_be_bytes()); // evolution
    buf.extend_from_slice(&color.to_be_bytes());
    buf.extend_from_slice(&3i16.to_be_bytes()); // skill count
    buf.push(1);
    buf.extend_from_slice(&(tid * 100 + 1).to_be_bytes());
    buf.push(2);
    buf.extend_from_slice(&(tid * 100 + 4).to_be_bytes());
    buf.push(3);
    buf.extend_from_slice(&tid.to_be_bytes());
    buf.extend_from_slice(&1i16.to_be_bytes()); // body_fashion_id
    buf.extend_from_slice(&0i16.to_be_bytes()); // hit_stun
    buf.extend_from_slice(&0i16.to_be_bytes()); // max_hit_stun
    buf.extend_from_slice(&0i16.to_be_bytes()); // auto_battle_rule
    buf.extend_from_slice(&0i16.to_be_bytes()); // body_fashion_color_id
    buf.extend_from_slice(&1i16.to_be_bytes()); // trailing live-wire i16
    buf
}

fn be_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_be_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]))
}

fn be_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
    ]))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Rewrite the attacker hero list inside a raw `SC_BATTLE_FIELD_INFO` (20101)
/// payload so the battle spawns the heroes the client deployed.
///
/// The live wire format of `pt_battle_hero` carries one more trailing i16
/// than the message schema knows about, so the payload cannot be re-encoded
/// through the struct.  The attacker hero section is therefore rebuilt with
/// byte surgery: recorded entries are reused for heroes that stay deployed,
/// freshly recruited heroes clone the first recorded entry into a free grid
/// cell, and benched recorded heroes are appended at the end so the recorded
/// action batches keep referencing heroes the client knows about.
fn count_at(raw: &[u8], offset: usize) -> Option<usize> {
    let count = be_i16(raw, offset)?;
    if count < 0 || count > 4096 {
        return None;
    }
    Some(count as usize)
}

fn be_i64(bytes: &[u8], offset: usize) -> Option<i64> {
    Some(i64::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
        *bytes.get(offset + 4)?,
        *bytes.get(offset + 5)?,
        *bytes.get(offset + 6)?,
        *bytes.get(offset + 7)?,
    ]))
}

/// Byte length of one live-wire pt_battle_hero, including its trailing star
/// level field.
fn hero_entry_len(raw: &[u8], start: usize) -> Option<usize> {
    let skills = count_at(raw, start.checked_add(40)?)?;
    54usize.checked_add(5usize.checked_mul(skills)?)
}

fn skip_assist_list(raw: &[u8], offset: usize) -> Option<usize> {
    let count = count_at(raw, offset)?;
    let mut offset = offset.checked_add(2)?;
    for _ in 0..count {
        // hero_tid i32 + hero_lv i16 + hero_evolution i16.
        offset = offset.checked_add(8)?;
        let skills = count_at(raw, offset)?;
        offset = offset
            .checked_add(2)?
            .checked_add(4usize.checked_mul(skills)?)?;
        if offset > raw.len() {
            return None;
        }
    }
    Some(offset)
}

fn build_actor_map(
    recorded: &[(i32, i32, Vec<u8>)],
    deployed: &[(i32, i32, i8)],
) -> HashMap<i32, (i32, i32, i32)> {
    let mut actor_map = HashMap::new();
    let mut assigned = HashSet::new();
    if deployed.is_empty() {
        return actor_map;
    }
    for (index, (old_id, old_tid, _)) in recorded.iter().enumerate() {
        let target = deployed
            .iter()
            .find(|hero| hero.0 == *old_id)
            .or_else(|| {
                deployed
                    .iter()
                    .find(|hero| hero.1 == *old_tid && !assigned.contains(&hero.0))
            })
            .or_else(|| deployed.iter().find(|hero| !assigned.contains(&hero.0)))
            .or_else(|| deployed.get(index % deployed.len()));
        if let Some((new_id, new_tid, _)) = target {
            actor_map.insert(*old_id, (*new_id, *old_tid, *new_tid));
            assigned.insert(*new_id);
        }
    }
    actor_map
}

#[derive(Default)]
struct FormationPatch {
    actor_map: HashMap<i32, (i32, i32, i32)>,
    added_heroes: Vec<(i32, i32)>,
}

/// Rebuild the attacker roster in an SC_BATTLE_FIELD_INFO raw payload. Unlike
/// the old behavior, captured bench heroes are removed rather than appended.
/// Their captured actor IDs are redirected to the client's ready formation in
/// subsequent actions and result messages.
fn patch_field_info_heroes(
    raw: &[u8],
    deployed: &[(i32, i32, i8)],
) -> Option<(Vec<u8>, FormationPatch)> {
    // battle_type i8 + field id i64 + player id i64, then player name.
    let name_len_offset = 1 + 8 + 8;
    let name_len = count_at(raw, name_len_offset)?;
    let name_end = name_len_offset.checked_add(2)?.checked_add(name_len)?;
    if name_end > raw.len() {
        return None;
    }
    let total_hp_offset = name_end.checked_add(2)?; // player level
    let count_off = total_hp_offset.checked_add(8)?.checked_add(2)?; // hp + avatar
    let recorded_count = count_at(raw, count_off)?;
    let mut offset = count_off.checked_add(2)?;

    let mut recorded: Vec<(i32, i32, Vec<u8>)> = Vec::with_capacity(recorded_count);
    for _ in 0..recorded_count {
        let start = offset;
        let len = hero_entry_len(raw, start)?;
        let end = start.checked_add(len)?;
        recorded.push((
            be_i32(raw, start)?,
            be_i32(raw, start.checked_add(4)?)?,
            raw.get(start..end)?.to_vec(),
        ));
        offset = end;
    }
    let mid_start = offset;

    // Attacker QTE and assists, followed by the complete defender block.
    offset = offset.checked_add(2)?;
    offset = skip_assist_list(raw, offset)?;
    offset = offset.checked_add(8)?; // defender player ID
    let defender_name_len = count_at(raw, offset)?;
    offset = offset
        .checked_add(2)?
        .checked_add(defender_name_len)?
        .checked_add(2 + 8 + 2)?; // name, level, HP, avatar
    let defender_count = count_at(raw, offset)?;
    offset = offset.checked_add(2)?;
    for _ in 0..defender_count {
        offset = offset.checked_add(hero_entry_len(raw, offset)?)?;
    }
    offset = offset.checked_add(2)?; // defender QTE
    offset = skip_assist_list(raw, offset)?;

    // hero_order is a count followed by (side i16, hero ID i32) pairs.
    let order_off = offset;
    let order_count = count_at(raw, order_off)?;
    let order_start = order_off.checked_add(2)?;
    let order_end = order_start.checked_add(order_count.checked_mul(6)?)?;
    let tail = raw.get(order_end..)?;

    let actor_map = build_actor_map(&recorded, deployed);
    let mut used_cells = Vec::new();
    let mut entries = Vec::with_capacity(deployed.len());
    for (id, tid, slot) in deployed {
        let cell = slot_cell(*slot, &used_cells);
        used_cells.push(cell);
        let template = recorded
            .iter()
            .find(|(recorded_id, recorded_tid, _)| recorded_id == id && recorded_tid == tid)
            .or_else(|| recorded.iter().find(|(recorded_id, _, _)| recorded_id == id))
            .or_else(|| recorded.iter().find(|(_, recorded_tid, _)| recorded_tid == tid));
        let (mut entry, source_tid) = match template {
            Some((_, source_tid, bytes)) => (bytes.clone(), *source_tid),
            None => (synth_hero_entry(*id, *tid, cell), *tid),
        };
        if entry.len() < 13 {
            return None;
        }
        entry[0..4].copy_from_slice(&id.to_be_bytes());
        entry[4..8].copy_from_slice(&tid.to_be_bytes());
        entry[9..11].copy_from_slice(&cell.0.to_be_bytes());
        entry[11..13].copy_from_slice(&cell.1.to_be_bytes());
        if source_tid != *tid {
            let skills = count_at(&entry, 40)?;
            for index in 0..skills {
                let skill_id_offset = 43usize.checked_add(index.checked_mul(5)?)?;
                let skill_id = be_i32(&entry, skill_id_offset)?;
                let remapped = remap_skill_id(skill_id, source_tid, *tid);
                entry[skill_id_offset..skill_id_offset + 4]
                    .copy_from_slice(&remapped.to_be_bytes());
            }
        }
        entries.push(entry);
    }

    let mut total_hp = 0i64;
    for entry in &entries {
        total_hp = total_hp.saturating_add(be_i64(entry, 13)?);
    }

    let active_ids: HashSet<i32> = deployed.iter().map(|(id, _, _)| *id).collect();
    let added_heroes = deployed
        .iter()
        .filter(|hero| !actor_map.values().any(|mapped| mapped.0 == hero.0))
        .map(|(id, tid, _)| (*id, *tid))
        .collect();

    let mut order_entries: Vec<Vec<u8>> = Vec::new();
    let mut ordered_player_ids = HashSet::new();
    for index in 0..order_count {
        let entry_offset = order_start.checked_add(index.checked_mul(6)?)?;
        let side = be_i16(raw, entry_offset)?;
        let hero_id = be_i32(raw, entry_offset.checked_add(2)?)?;
        if side == 1 {
            if let Some((id, _, _)) = actor_map.get(&hero_id) {
                if active_ids.contains(id) && ordered_player_ids.insert(*id) {
                    let mut entry = Vec::with_capacity(6);
                    entry.extend_from_slice(&1i16.to_be_bytes());
                    entry.extend_from_slice(&id.to_be_bytes());
                    order_entries.push(entry);
                }
            }
        } else {
            order_entries.push(raw.get(entry_offset..entry_offset.checked_add(6)?)?.to_vec());
        }
    }
    for (id, _, _) in deployed {
        if ordered_player_ids.insert(*id) {
            let mut entry = Vec::with_capacity(6);
            entry.extend_from_slice(&1i16.to_be_bytes());
            entry.extend_from_slice(&id.to_be_bytes());
            order_entries.push(entry);
        }
    }

    let mut out = Vec::with_capacity(raw.len() + 64);
    out.extend_from_slice(raw.get(..total_hp_offset)?);
    out.extend_from_slice(&total_hp.to_be_bytes());
    out.extend_from_slice(raw.get(total_hp_offset.checked_add(8)?..count_off)?);
    out.extend_from_slice(&(entries.len() as i16).to_be_bytes());
    for entry in &entries {
        out.extend_from_slice(entry);
    }
    out.extend_from_slice(raw.get(mid_start..order_off)?);
    out.extend_from_slice(&(order_entries.len() as i16).to_be_bytes());
    for entry in &order_entries {
        out.extend_from_slice(entry);
    }
    out.extend_from_slice(tail);

    Some((
        out,
        FormationPatch {
            actor_map,
            added_heroes,
        },
    ))
}

fn patch_enter_formation(
    group: &mut TemplateGroup,
    deployed: &[(i32, i32, i8)],
) -> FormationPatch {
    let mut patch = FormationPatch::default();
    for response in group.responses.iter_mut() {
        if response.cmd != 20101 {
            continue;
        }
        let Some(raw) = response.payload_hex.as_deref().and_then(decode_payload_hex) else {
            tracing::warn!("Cannot patch battle field info without a raw payload");
            continue;
        };
        if let Some((patched, roster)) = patch_field_info_heroes(&raw, deployed) {
            info!(
                deployed = ?deployed,
                bytes = patched.len(),
                "Battle field info patched with ready formation"
            );
            response.payload_hex = Some(to_hex(&patched));
            patch = roster;
        } else {
            tracing::warn!(
                deployed = ?deployed,
                "Could not parse captured battle field info; retaining its original roster"
            );
        }
    }
    patch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retreat_result_has_no_victory_rewards() {
        let result = retreat_result(4);
        let decoded = crate::dispatch::dispatch_cmd(20106, &result.encode()).unwrap();
        assert_eq!(decoded["result"], 3);
        assert_eq!(decoded["award"], json!([]));
        assert_eq!(decoded["detail_item_award"], json!([]));
        assert_eq!(decoded["player_exp"], 0);
        assert_eq!(decoded["hero_exp"], 0);
        assert_eq!(decoded["round"], 4);
    }

    #[test]
    fn action_and_effect_actor_ids_follow_the_ready_roster() {
        let mut actor_map = HashMap::new();
        actor_map.insert(7, (42, 1305, 1006));
        actor_map.insert(8, (43, 1110, 1304));
        let mut response = TemplateResponse {
            cmd: 20103,
            decoded: Some(json!({
                "side": 1,
                "hero_id": 7,
                "skill_id": 130501,
                "target_side": 1,
                "target_id": 8,
                "target_list": [
                    {"side": 1, "hero_id": 8, "effect_list": []},
                    {"side": 2, "hero_id": 99, "effect_list": []}
                ],
                "hero_list": [],
                "result_list": [
                    {"qte_val": 0, "hero_list": [
                        {"side": 1, "hero_id": 7, "effect_list": []}
                    ], "has_extra_call": 0, "is_final_hit": 0}
                ],
                "sync_word": 123
            })),
            payload_hex: Some("deadbeef".to_owned()),
        };

        remap_battle_response(&mut response, &actor_map);
        let decoded = response.decoded.unwrap();
        assert_eq!(decoded["hero_id"], 42);
        assert_eq!(decoded["skill_id"], 100601);
        assert_eq!(decoded["target_id"], 43);
        assert_eq!(decoded["target_list"][0]["hero_id"], 43);
        assert_eq!(decoded["target_list"][1]["hero_id"], 99);
        assert_eq!(decoded["result_list"][0]["hero_list"][0]["hero_id"], 42);
        assert!(response.payload_hex.is_none());
    }

    fn push_i16(raw: &mut Vec<u8>, value: i16) {
        raw.extend_from_slice(&value.to_be_bytes());
    }

    fn push_i32(raw: &mut Vec<u8>, value: i32) {
        raw.extend_from_slice(&value.to_be_bytes());
    }

    fn push_i64(raw: &mut Vec<u8>, value: i64) {
        raw.extend_from_slice(&value.to_be_bytes());
    }

    fn field_info_with_two_attackers() -> Vec<u8> {
        let mut raw = Vec::new();
        raw.push(1); // battle_type
        push_i64(&mut raw, 1001); // field id
        push_i64(&mut raw, 10); // attacker player id
        push_i16(&mut raw, 1);
        raw.push(b'p');
        push_i16(&mut raw, 1); // player level
        push_i64(&mut raw, 1304); // original total HP
        push_i16(&mut raw, 0); // avatar
        push_i16(&mut raw, 2); // attacker count
        raw.extend_from_slice(&synth_hero_entry(7, 1110, (2, 1)));
        raw.extend_from_slice(&synth_hero_entry(8, 1305, (1, 2)));
        push_i16(&mut raw, 0); // attacker qte
        push_i16(&mut raw, 0); // attacker assists

        push_i64(&mut raw, 20); // defender player id
        push_i16(&mut raw, 1);
        raw.push(b'd');
        push_i16(&mut raw, 1); // player level
        push_i64(&mut raw, 0); // total HP
        push_i16(&mut raw, 0); // avatar
        push_i16(&mut raw, 0); // defender count
        push_i16(&mut raw, 0); // defender qte
        push_i16(&mut raw, 0); // defender assists

        push_i16(&mut raw, 3); // order count
        push_i16(&mut raw, 1);
        push_i32(&mut raw, 7);
        push_i16(&mut raw, 1);
        push_i32(&mut raw, 8);
        push_i16(&mut raw, 2);
        push_i32(&mut raw, 99);
        raw.push(20); // max round
        raw.push(0); // auto
        raw.push(0); // replay
        push_i32(&mut raw, 71510000);
        push_i16(&mut raw, 0); // scene skills
        raw.push(1); // enter result
        push_i16(&mut raw, 0); // forces skills
        push_i16(&mut raw, 0); // forces energy
        raw
    }

    #[test]
    fn attacker_trigger_heroes_are_not_reintroduced_after_field_entry() {
        let mut actor_map = HashMap::new();
        actor_map.insert(7, (42, 1305, 1006));
        let mut response = TemplateResponse {
            cmd: 20118,
            decoded: Some(json!({
                "side": 1,
                "hero_list": [{"id": 999, "tid": 1999}],
                "story_id": 1,
                "talk_id": 2,
                "sync_word": 10
            })),
            payload_hex: Some("deadbeef".to_owned()),
        };

        remap_battle_response(&mut response, &actor_map);
        assert_eq!(response.decoded.as_ref().unwrap()["hero_list"], json!([]));
        assert!(response.payload_hex.is_none());
    }

    #[test]
    fn selected_ready_team_wins_over_the_captured_team() {
        let hero = |hero_id, tid| crate::messages::pt_formation_hero_info {
            pos: 1,
            is_captain: 1,
            hero_id,
            tid,
            hero_source: 1,
        };
        let formation = vec![
            crate::messages::pt_hero_formation {
                team_id: 1001,
                formation_id: 1,
                is_ready: 1,
                name: "capture team".to_owned(),
                formation_hero_list: vec![hero(7, 1110)],
                assist_fight_list: Vec::new(),
                pet_id: 0,
            },
            crate::messages::pt_hero_formation {
                team_id: 1002,
                formation_id: 2,
                is_ready: 0,
                name: "selected team".to_owned(),
                formation_hero_list: vec![hero(42, 1006)],
                assist_fight_list: Vec::new(),
                pet_id: 0,
            },
        ];
        assert_eq!(deployed_heroes(&formation, Some(1002)), vec![(42, 1006, 1)]);
    }

    #[test]
    fn field_info_removes_benched_capture_heroes() {
        let raw = field_info_with_two_attackers();
        let deployed = [(42, 1006, 1)];
        let (patched, roster) = patch_field_info_heroes(&raw, &deployed).unwrap();
        let decoded = crate::messages::SC_BATTLE_FIELD_INFO::decode(&patched);

        assert_eq!(decoded.att_info.hero_list.len(), 1);
        assert_eq!(decoded.att_info.hero_list[0].id, 42);
        assert_eq!(decoded.att_info.hero_list[0].tid, 1006);
        assert_eq!(decoded.att_info.total_hp, "529");
        assert_eq!(decoded.hero_order.len(), 2);
        assert_eq!(decoded.hero_order[0].key, 1);
        assert_eq!(decoded.hero_order[0].value, 42);
        assert_eq!(decoded.hero_order[1].key, 2);
        assert_eq!(decoded.hero_order[1].value, 99);
        assert_eq!(roster.actor_map.get(&7), Some(&(42, 1110, 1006)));
        assert_eq!(roster.actor_map.get(&8), Some(&(42, 1305, 1006)));
    }
}
