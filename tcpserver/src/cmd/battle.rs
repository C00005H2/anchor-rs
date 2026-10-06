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
        CS_BATTLE_AUTO, CS_BATTLE_FIELD_ENTER, CS_BATTLE_FORCES_SKILL, CS_BATTLE_START,
        CS_BATTLE_SYNC, CS_BATTLE_USE_SKILL, CS_BATTLE_VIDEO_END, CS_HERO_AUTO_RULE_CHANGE,
        SC_BATTLE_FORCES_SKILL_ENERGY, SC_BATTLE_NONE, SC_BATTLE_RESULT, SC_BATTLE_USE_SKILL,
        SC_HERO_AUTO_RULE_CHANGE,
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
const DELAYED_SKILL_LOOKAHEAD: usize = 5;

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

fn remap_skill_id(skill_id: i32, old_tid: i32, new_tid: i32) -> i32 {
    // Captured basic attacks sometimes use the hero tid itself as the skill id.
    if skill_id == old_tid {
        new_tid
    } else if skill_id >= 0 && skill_id / 100 == old_tid {
        new_tid.saturating_mul(100).saturating_add(skill_id % 100)
    } else {
        skill_id
    }
}

fn battle_skill_ack(
    response: &TemplateResponse,
    actor_map: &HashMap<i32, (i32, i32, i32)>,
) -> Option<BattlePendingSkill> {
    if response.cmd != 20115 {
        return None;
    }
    let (hero_id, skill_id, result) = if let Some(decoded) = response.decoded.as_ref() {
        (
            decoded.get("hero_id")?.as_i64()? as i32,
            decoded.get("skill_id")?.as_i64()? as i32,
            decoded.get("result")?.as_i64()? as i8,
        )
    } else {
        let raw = response.payload_hex.as_deref().and_then(decode_payload_hex)?;
        if raw.len() < 9 {
            return None;
        }
        (be_i32(&raw, 0)?, be_i32(&raw, 4)?, raw[8] as i8)
    };
    if result != 1 || hero_id == 0 {
        return None;
    }

    // Captured acknowledgements still use capture actor ids and skill ids.
    // An ack rewritten by handle_battle_use_skill has no raw payload and is
    // already expressed in the currently deployed formation.
    if response.payload_hex.is_some() {
        if let Some((current_id, old_tid, new_tid)) = actor_map.get(&hero_id).copied() {
            return Some(BattlePendingSkill {
                hero_id: current_id,
                skill_id: remap_skill_id(skill_id, old_tid, new_tid),
            });
        }
        if !actor_map.is_empty()
            && !actor_map.values().any(|(current_id, _, _)| *current_id == hero_id)
        {
            return None;
        }
    }
    Some(BattlePendingSkill { hero_id, skill_id })
}

/// Return the acting side, deployed hero id, and skill id after applying the
/// current roster mapping. Skill acknowledgements are only safe to pair with
/// an action carrying the same hero and skill; rewriting those ids on an
/// unrelated recorded action produces the visible "ghost skill" animation.
fn battle_action_signature(
    response: &TemplateResponse,
    actor_map: &HashMap<i32, (i32, i32, i32)>,
) -> Option<(i32, i32, i32)> {
    if response.cmd != 20103 {
        return None;
    }
    let mut response = response.clone();
    if !actor_map.is_empty() {
        remap_battle_response(&mut response, actor_map);
    }
    if let Some(decoded) = response.decoded.as_ref() {
        if let (Some(side), Some(hero_id), Some(skill_id)) = (
            decoded.get("side").and_then(Value::as_i64),
            decoded.get("hero_id").and_then(Value::as_i64),
            decoded.get("skill_id").and_then(Value::as_i64),
        ) {
            return Some((side as i32, hero_id as i32, skill_id as i32));
        }
    }
    let raw = response.payload_hex.as_deref().and_then(decode_payload_hex)?;
    Some((
        *raw.first()? as i32,
        be_i32(&raw, 1)?,
        be_i32(&raw, 5)?,
    ))
}

fn remap_side_heroes(value: &mut Value, actor_map: &HashMap<i32, (i32, i32, i32)>) {
    match value {
        Value::Object(object) => {
            let side = object.get("side").and_then(Value::as_i64);
            let old_id = object.get("hero_id").and_then(Value::as_i64);
            if side == Some(1) {
                if let Some(old_id) = old_id {
                    if let Some((new_id, _, _)) = actor_map.get(&(old_id as i32)) {
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
                            let Some((new_id, _, _)) = actor_map.get(&(old_id as i32)) else {
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
    let mut seen_attackers = HashSet::new();
    items.retain_mut(|item| {
        let Some(object) = item.as_object_mut() else { return true };
        if object.get("key").and_then(Value::as_i64) != Some(1) {
            return true;
        }
        let Some(old_id) = object.get("value").and_then(Value::as_i64) else {
            return false;
        };
        let Some((new_id, _, _)) = actor_map.get(&(old_id as i32)) else {
            return false;
        };
        if !seen_attackers.insert(*new_id) {
            return false;
        }
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
                                actor_map.get(&(old_id as i32)).copied()
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
                            } else {
                                // An unmapped captured attacker was benched;
                                // do not let an ID collision make it appear active.
                                object.insert("hero_id".to_owned(), json!(0));
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
                            if let Some((new_id, _, _)) = actor_map.get(&(old_id as i32)) {
                                object.insert("target_id".to_owned(), json!(new_id));
                            } else {
                                object.insert("target_id".to_owned(), json!(0));
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
            20115 => {
                // A raw-backed response is still in capture coordinates. The
                // manual handler clears the raw payload after writing the
                // selected hero and skill, so it must not be remapped twice.
                if response.payload_hex.is_some() {
                    let Some(object) = decoded.as_object_mut() else { return };
                    let Some(old_id) = object.get("hero_id").and_then(Value::as_i64) else {
                        return;
                    };
                    let Some((new_id, old_tid, new_tid)) =
                        actor_map.get(&(old_id as i32)).copied()
                    else {
                        object.insert("hero_id".to_owned(), json!(0));
                        response.payload_hex = None;
                        return;
                    };
                    object.insert("hero_id".to_owned(), json!(new_id));
                    if let Some(skill_id) = object.get("skill_id").and_then(Value::as_i64) {
                        object.insert(
                            "skill_id".to_owned(),
                            json!(remap_skill_id(skill_id as i32, old_tid, new_tid)),
                        );
                    }
                    response.payload_hex = None;
                }
            }
            20129 => {
                if let Some(object) = decoded.as_object_mut() {
                    if object.get("hero_side").and_then(Value::as_i64) == Some(1) {
                        if let Some(old_id) = object.get("hero_id").and_then(Value::as_i64) {
                            if let Some((new_id, _, new_tid)) =
                                actor_map.get(&(old_id as i32)).copied()
                            {
                                object.insert("hero_id".to_owned(), json!(new_id));
                                object.insert("hero_tid".to_owned(), json!(new_tid));
                            } else {
                                object.insert("hero_id".to_owned(), json!(0));
                                object.insert("hero_tid".to_owned(), json!(0));
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
                                actor_map.get(&(old_id as i32)).copied()
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
                            let Some((new_id, _, _)) = actor_map.get(&(old_id as i32)) else {
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
            let Some((new_id, old_tid, new_tid)) = actor_map.get(&old_id).copied() else {
                raw[1..5].copy_from_slice(&0i32.to_be_bytes());
                raw[5..9].copy_from_slice(&0i32.to_be_bytes());
                response.payload_hex = Some(to_hex(&raw));
                return;
            };
            raw[1..5].copy_from_slice(&new_id.to_be_bytes());
            if let Some(skill_id) = be_i32(&raw, 5) {
                raw[5..9].copy_from_slice(&remap_skill_id(skill_id, old_tid, new_tid).to_be_bytes());
            }
        }
        20115 if raw.len() >= 17 => {
            let Some(old_id) = be_i32(&raw, 0) else { return };
            let Some((new_id, old_tid, new_tid)) = actor_map.get(&old_id).copied() else {
                raw[..4].copy_from_slice(&0i32.to_be_bytes());
                raw[4..8].copy_from_slice(&0i32.to_be_bytes());
                response.payload_hex = Some(to_hex(&raw));
                return;
            };
            let Some(skill_id) = be_i32(&raw, 4) else { return };
            response.decoded = Some(json!({
                "hero_id": new_id,
                "skill_id": remap_skill_id(skill_id, old_tid, new_tid),
                "result": raw[8] as i8,
                "skill_soul": be_i16(&raw, 9).unwrap_or_default(),
                "rage": be_i16(&raw, 11).unwrap_or_default(),
                "sync_word": be_i32(&raw, 13).unwrap_or_default(),
            }));
            response.payload_hex = None;
            return;
        }
        20129 if raw.len() >= 10 && raw[0] == 1 => {
            let Some(old_id) = be_i32(&raw, 1) else { return };
            let Some((new_id, _, new_tid)) = actor_map.get(&old_id).copied() else {
                raw[1..5].copy_from_slice(&0i32.to_be_bytes());
                raw[5..9].copy_from_slice(&0i32.to_be_bytes());
                response.payload_hex = Some(to_hex(&raw));
                return;
            };
            raw[1..5].copy_from_slice(&new_id.to_be_bytes());
            raw[5..9].copy_from_slice(&new_tid.to_be_bytes());
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

/// Captures show that a 20120 sync can carry an ordinary player action between
/// a skill acknowledgement and the action that executes that skill. Only a
/// 20104 action batch (or an action-bearing 20108 batch) consumes the queue.
fn request_consumes_pending_skill(request_cmd: Option<u32>) -> bool {
    matches!(request_cmd, Some(20104) | Some(20108))
}

/// Remap, encode and account for one server-response group.
async fn encode_group(
    ctx: &Arc<Mutex<ConnectionContext>>,
    mut group: TemplateGroup,
    request_cmd: Option<u32>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let (cursor, added, actor_map, active_heroes, suppress_attacker) = {
        let connection = ctx.lock().await;
        (
            connection.replay_cursor.clone(),
            connection.battle_added_heroes.clone(),
            connection.battle_actor_map.clone(),
            connection.battle_active_heroes.clone(),
            connection.formation_received && connection.battle_active_heroes.is_empty(),
        )
    };
    let active_ids: HashSet<i32> = active_heroes.iter().map(|(id, _, _)| *id).collect();
    for response in &mut group.responses {
        if !actor_map.is_empty() || suppress_attacker {
            remap_battle_response(response, &actor_map);
        }
    }

    // Replay a captured skill ack only when this connection has a matching,
    // validated skill request pending. Some captures deliver the ack in a
    // later 20104/20120 group; duplicate copies in that group are collapsed.
    // Without a pending request these recorded acks would become ghost casts
    // in sessions replayed by an auto-battle client.
    let (pending_skills, pending_skill) = {
        let connection = ctx.lock().await;
        let pending_skills = connection.battle_pending_skills.iter().copied().collect::<Vec<_>>();
        let pending_skill = if request_consumes_pending_skill(request_cmd) {
            pending_skills.first().copied()
        } else {
            None
        };
        (pending_skills, pending_skill)
    };
    let mut acknowledged_skills = Vec::new();
    group.responses.retain(|response| {
        if response.cmd != 20115 {
            return true;
        }
        let Some(skill) = battle_skill_ack(response, &actor_map) else {
            return false;
        };
        pending_skills.contains(&skill) && {
            if acknowledged_skills.contains(&skill) {
                false
            } else {
                acknowledged_skills.push(skill);
                true
            }
        }
    });

    // A queued manual skill is consumed only by its matching recorded action.
    // Never rewrite an unrelated actor/action's identifiers: the effect list
    // still belongs to the recorded move and would otherwise create a ghost
    // cast (often on the final action immediately before victory).
    let applied_pending_skill = pending_skill.is_some_and(|skill| {
        group.responses.iter().any(|response| {
            // The group was already remapped above; do not apply the mapping
            // a second time to deployed ids.
            battle_action_signature(response, &HashMap::new()).is_some_and(
                |(side, hero_id, skill_id)| {
                    side == 1
                        && hero_id == skill.hero_id
                        && skill_id == skill.skill_id
                        && active_ids.contains(&hero_id)
                },
            )
        })
    });
    if !active_heroes.is_empty() {
        group.responses.retain(|response| match response.cmd {
            20103 => {
                let side = response
                    .decoded
                    .as_ref()
                    .and_then(|decoded| decoded.get("side"))
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        response
                            .payload_hex
                            .as_deref()
                            .and_then(decode_payload_hex)
                            .and_then(|raw| raw.first().copied())
                            .map(i64::from)
                    });
                if side != Some(1) {
                    return true;
                }
                let hero_id = response
                    .decoded
                    .as_ref()
                    .and_then(|decoded| decoded.get("hero_id"))
                    .and_then(Value::as_i64)
                    .map(|id| id as i32)
                    .or_else(|| {
                        response
                            .payload_hex
                            .as_deref()
                            .and_then(decode_payload_hex)
                            .and_then(|raw| be_i32(&raw, 1))
                    });
                hero_id.is_some_and(|id| active_ids.contains(&id))
            }
            20129 => {
                let side = response
                    .decoded
                    .as_ref()
                    .and_then(|decoded| decoded.get("hero_side"))
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        response
                            .payload_hex
                            .as_deref()
                            .and_then(decode_payload_hex)
                            .and_then(|raw| raw.first().copied())
                            .map(i64::from)
                    });
                if side != Some(1) {
                    return true;
                }
                response
                    .decoded
                    .as_ref()
                    .and_then(|decoded| decoded.get("hero_id"))
                    .and_then(Value::as_i64)
                    .map(|id| id as i32)
                    .or_else(|| {
                        response
                            .payload_hex
                            .as_deref()
                            .and_then(decode_payload_hex)
                            .and_then(|raw| be_i32(&raw, 1))
                    })
                    .is_some_and(|id| active_ids.contains(&id))
            }
            20115 => response
                .decoded
                .as_ref()
                .and_then(|decoded| decoded.get("hero_id"))
                .and_then(Value::as_i64)
                .map(|id| active_ids.contains(&(id as i32)))
                .unwrap_or_else(|| {
                    response
                        .payload_hex
                        .as_deref()
                        .and_then(decode_payload_hex)
                        .and_then(|raw| be_i32(&raw, 0))
                        .is_some_and(|id| active_ids.contains(&id))
                }),
            _ => true,
        });
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
        if group.responses.iter().any(|response| response.cmd == 20103) {
            // Remember the last served action so a sync poll can re-send it:
            // the official server repeats the current action on 20120.
            if let Some(action) = packets
                .iter()
                .rev()
                .find(|packet| {
                    packet.len() >= 6
                        && u32::from_be_bytes([packet[2], packet[3], packet[4], packet[5]])
                            == 20103
                })
                .cloned()
            {
                connection.battle_last_action = Some((action, connection.battle_sync_word));
            }
        }
        if applied_pending_skill {
            connection.battle_pending_skills.pop_front();
        }
        if group.responses.iter().any(|response| response.cmd == 20106) {
            connection.battle_result_served = true;
            connection.battle_active = false;
            connection.battle_pending_skills.clear();
            connection.battle_sync_advance_pending = false;
            connection.battle_auto_resume_sync_word = None;
            connection.battle_terminal_result = packets
                .iter()
                .find(|packet| {
                    packet.len() >= 6
                        && u32::from_be_bytes([packet[2], packet[3], packet[4], packet[5]])
                            == 20106
                })
                .cloned();
        }
    }
    Ok(packets)
}

async fn encode_step(
    ctx: &Arc<Mutex<ConnectionContext>>,
    step: &BattleStep,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    encode_group(ctx, step.as_group(), Some(step.request_cmd)).await
}

/// Pick the next unconsumed response step without replaying stale skill/sync
/// events out of order. A video-end advances past optional skill and sync
/// requests the client did not send; a sync poll only consumes a sync step if
/// it is next in the timeline.
async fn take_step(
    ctx: &Arc<Mutex<ConnectionContext>>,
    request_cmd: u32,
) -> Option<BattleStep> {
    let mut connection = ctx.lock().await;
    if !connection.battle_active || connection.battle_result_served {
        return None;
    }
    let chosen = connection.battle_session_chosen?;
    let session = load_session(chosen)?;
    for (index, step) in session.steps.iter().enumerate() {
        let consumed = connection
            .battle_step_consumed
            .get(index)
            .copied()
            .unwrap_or(true);
        if consumed {
            continue;
        }

        if request_cmd == 20104 && matches!(step.request_cmd, 20108 | 20120) {
            // The player did not issue this captured skill/sync request. Mark
            // it stale rather than emitting a ghost acknowledgement much
            // later when the next video-end arrives.
            if let Some(flag) = connection.battle_step_consumed.get_mut(index) {
                *flag = true;
            }
            continue;
        }
        if step.request_cmd != request_cmd {
            // Do not skip a pending action/skill step to reach a future sync
            // response. The caller can synthesize the appropriate fallback.
            if request_cmd == 20120 {
                return None;
            }
            continue;
        }
        if let Some(flag) = connection.battle_step_consumed.get_mut(index) {
            *flag = true;
        }
        if step.responses.is_empty() {
            continue;
        }
        return Some(step.clone());
    }
    None
}

/// Consume the next captured skill response only when it actually matches the
/// requested deployed hero and skill. Empty captured skill batches are safe to
/// skip; a non-empty mismatch is left for ordinary timeline advancement.
async fn take_matching_skill_step(
    ctx: &Arc<Mutex<ConnectionContext>>,
    requested: BattlePendingSkill,
) -> Option<BattleStep> {
    let mut connection = ctx.lock().await;
    if !connection.battle_active || connection.battle_result_served {
        return None;
    }
    let chosen = connection.battle_session_chosen?;
    let session = load_session(chosen)?;
    let actor_map = connection.battle_actor_map.clone();
    let active_ids: HashSet<i32> = connection
        .battle_active_heroes
        .iter()
        .map(|(hero_id, _, _)| *hero_id)
        .collect();

    for (index, step) in session.steps.iter().enumerate() {
        if connection
            .battle_step_consumed
            .get(index)
            .copied()
            .unwrap_or(true)
        {
            continue;
        }
        match step.request_cmd {
            20108 => {
                if step.responses.is_empty() {
                    if let Some(flag) = connection.battle_step_consumed.get_mut(index) {
                        *flag = true;
                    }
                    continue;
                }
                let matching_ack = step.responses.iter().any(|response| {
                    battle_skill_ack(response, &actor_map) == Some(requested)
                });
                let matching_action = step.responses.iter().any(|response| {
                    battle_action_signature(response, &actor_map).is_some_and(
                        |(side, hero_id, skill_id)| {
                            side == 1
                                && hero_id == requested.hero_id
                                && skill_id == requested.skill_id
                                && active_ids.contains(&hero_id)
                        },
                    )
                });
                if matching_ack || matching_action {
                    if let Some(flag) = connection.battle_step_consumed.get_mut(index) {
                        *flag = true;
                    }
                    return Some(step.clone());
                }
                return None;
            }
            // Do not jump over an earlier action or sync step just to find a
            // matching skill acknowledgement later in the recording.
            20104 | 20120 => return None,
            _ => {}
        }
    }
    None
}

/// Is the next active player action in the recording exactly the requested
/// skill? This gates synthetic acknowledgements when the chosen recording did
/// not capture a 20108 batch, so arbitrary taps cannot rewrite unrelated
/// attacks or queue a skill to fire on a later hero's turn.
fn next_scripted_action_matches_skill(
    connection: &ConnectionContext,
    requested: BattlePendingSkill,
) -> bool {
    let actor_map = &connection.battle_actor_map;
    let active_ids: HashSet<i32> = connection
        .battle_active_heroes
        .iter()
        .map(|(hero_id, _, _)| *hero_id)
        .collect();
    if active_ids.is_empty() {
        return false;
    }

    let first_active_player_action = |responses: &[TemplateResponse]| {
        for response in responses {
            let Some((side, hero_id, skill_id)) =
                battle_action_signature(response, actor_map)
            else {
                continue;
            };
            if side != 1 || !active_ids.contains(&hero_id) {
                continue;
            }
            return Some((hero_id, skill_id));
        }
        None
    };

    if let Some(chosen) = connection.battle_session_chosen {
        let Some(session) = load_session(chosen) else {
            return false;
        };
        for (index, step) in session.steps.iter().enumerate() {
            if connection
                .battle_step_consumed
                .get(index)
                .copied()
                .unwrap_or(true)
                || !matches!(step.request_cmd, 20104 | 20108)
            {
                continue;
            }
            if let Some((hero_id, skill_id)) = first_active_player_action(&step.responses) {
                return hero_id == requested.hero_id && skill_id == requested.skill_id;
            }
        }
        return false;
    }

    let Ok(script) = TemplateFile::load(VIDEO_END_DATA) else {
        return false;
    };
    for index in connection.battle_script_index..script.group_count() {
        let Some(group) = script.group(index) else {
            continue;
        };
        if let Some((hero_id, skill_id)) = first_active_player_action(&group.responses) {
            return hero_id == requested.hero_id && skill_id == requested.skill_id;
        }
    }
    false
}

/// Some captures return a skill acknowledgement with a later video-end rather
/// than the original 20108 request. Accept that delayed response only when it
/// is for this exact requested skill and a matching deployed action follows it.
fn captured_skill_ack_precedes_matching_action(
    connection: &ConnectionContext,
    requested: BattlePendingSkill,
) -> bool {
    let actor_map = &connection.battle_actor_map;
    let active_ids: HashSet<i32> = connection
        .battle_active_heroes
        .iter()
        .map(|(hero_id, _, _)| *hero_id)
        .collect();
    if active_ids.is_empty() {
        return false;
    }

    let mut saw_matching_ack = false;
    let mut has_matching_action_after_ack = |responses: &[TemplateResponse]| {
        for response in responses {
            if battle_skill_ack(response, actor_map) == Some(requested) {
                saw_matching_ack = true;
            }
            if saw_matching_ack
                && battle_action_signature(response, actor_map).is_some_and(
                    |(side, hero_id, skill_id)| {
                        side == 1
                            && hero_id == requested.hero_id
                            && skill_id == requested.skill_id
                            && active_ids.contains(&hero_id)
                    },
                )
            {
                return true;
            }
        }
        false
    };

    if let Some(chosen) = connection.battle_session_chosen {
        let Some(session) = load_session(chosen) else {
            return false;
        };
        let start = connection
            .battle_step_consumed
            .iter()
            .position(|consumed| !*consumed)
            .unwrap_or(session.steps.len());
        for (index, step) in session
            .steps
            .iter()
            .enumerate()
            .skip(start)
            .take(DELAYED_SKILL_LOOKAHEAD)
        {
            if connection
                .battle_step_consumed
                .get(index)
                .copied()
                .unwrap_or(true)
                || !matches!(step.request_cmd, 20104 | 20108 | 20120)
            {
                continue;
            }
            if has_matching_action_after_ack(&step.responses) {
                return true;
            }
        }
        return false;
    }

    let Ok(script) = TemplateFile::load(VIDEO_END_DATA) else {
        return false;
    };
    for index in connection
        .battle_script_index
        ..connection
            .battle_script_index
            .saturating_add(DELAYED_SKILL_LOOKAHEAD)
            .min(script.group_count())
    {
        let Some(group) = script.group(index) else {
            continue;
        };
        if has_matching_action_after_ack(&group.responses) {
            return true;
        }
    }
    false
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
        // Prefer recordings that actually finish with SC_BATTLE_RESULT: a
        // session captured from an abandoned battle would otherwise leave the
        // match with no way to reach the result screen.
        let mut chosen = None;
        let mut fallback = None;
        let mut exact_any = None;
        let mut fallback_any = None;
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
            let has_result = session.steps.iter().any(|step| step.contains(20106));
            if fallback_any.is_none() {
                fallback_any = Some(index);
            }
            if matches && exact_any.is_none() {
                exact_any = Some(index);
            }
            if !has_result {
                continue;
            }
            if matches {
                chosen = Some(index);
                break;
            }
            if fallback.is_none() {
                fallback = Some(index);
            }
        }
        let index = chosen
            .or(fallback)
            .or(exact_any)
            .or(fallback_any)
            .unwrap_or(start);
        let session = load_session(index).unwrap_or_default();
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
            recorded_field_id = ?session.field_id.as_deref(),
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
        if deployed.is_empty() {
            // The client deployed nobody: keep the recorded roster on the wire
            // and use it as the active lineup, so the battle stays playable
            // instead of filtering away every player-side action.
            patch_enter_field_id(&mut group, &request.battle_field_id);
            let recorded = recorded_roster(&group);
            let mut connection = ctx.lock().await;
            connection.battle_active_heroes = recorded;
        } else {
            let roster = patch_enter_formation(&mut group, &deployed);
            patch_enter_field_id(&mut group, &request.battle_field_id);
            let mut connection = ctx.lock().await;
            connection.battle_active_heroes = deployed;
            connection.battle_actor_map = roster.actor_map;
            connection.battle_added_heroes = roster.added_heroes;
        }
    } else {
        patch_enter_field_id(&mut group, &request.battle_field_id);
        // No formation was ever reported: track the recorded attackers so
        // manual skill taps can still be mapped to a valid hero id.
        let recorded = recorded_roster(&group);
        let mut connection = ctx.lock().await;
        connection.battle_active_heroes = recorded;
    }

    encode_group(&ctx, group, None).await
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
        if deployed.is_empty() {
            // The client deployed nobody: keep the recorded roster on the wire
            // and use it as the active lineup, so the battle stays playable
            // instead of filtering away every player-side action.
            patch_enter_field_id(&mut group, &request.battle_field_id);
            let recorded = recorded_roster(&group);
            let mut connection = ctx.lock().await;
            connection.battle_active_heroes = recorded;
        } else {
            let roster = patch_enter_formation(&mut group, &deployed);
            patch_enter_field_id(&mut group, &request.battle_field_id);
            let mut connection = ctx.lock().await;
            connection.battle_active_heroes = deployed;
            connection.battle_actor_map = roster.actor_map;
            connection.battle_added_heroes = roster.added_heroes;
        }
    } else {
        patch_enter_field_id(&mut group, &request.battle_field_id);
        // No formation was ever reported: track the recorded attackers so
        // manual skill taps can still be mapped to a valid hero id.
        let recorded = recorded_roster(&group);
        let mut connection = ctx.lock().await;
        connection.battle_active_heroes = recorded;
    }

    info!(
        battle_type = request.battle_type,
        battle_field_id = %request.battle_field_id,
        "Battle field entered"
    );
    encode_group(&ctx, group, None).await
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
    // later toggles are acknowledged with SC_BATTLE_AUTO, as in the captures.
    // When auto is enabled again, also push the next action: some clients do
    // not issue the captured follow-up 20104 after switching modes. If they do,
    // handle_battle_video_end recognizes that sync as already answered.
    let repeat = {
        let mut connection = ctx.lock().await;
        let repeat = connection.battle_auto_served;
        connection.battle_auto_served = true;
        repeat
    };
    if repeat {
        group.responses.retain(|response| response.cmd == 20114);
        if !group.responses.iter().any(|response| response.cmd == 20114) {
            if let Some(ack) = TemplateFile::load(AUTO_DATA)
                .ok()
                .and_then(|file| file.first_group().cloned())
                .and_then(|group| {
                    group
                        .responses
                        .into_iter()
                        .find(|response| response.cmd == 20114)
                })
            {
                group.responses.push(ack);
            }
        }
    }

    // Echo the requested auto-battle flag in SC_BATTLE_AUTO.
    for response in group.responses.iter_mut() {
        if response.cmd == 20114 {
            if let Some(object) = response
                .decoded
                .as_mut()
                .and_then(|decoded| decoded.as_object_mut())
            {
                object.insert("is_auto".to_owned(), serde_json::json!(request.is_auto));
                response.payload_hex = None;
            }
        }
    }

    let mut response_request_cmd = Some(20113);
    let mut legacy_resume = false;
    let mut resume_sync_word = None;
    if repeat && request.is_auto != 0 {
        let current_sync_word = ctx.lock().await.battle_sync_word;
        if let Some(step) = take_step(&ctx, 20104).await {
            group.responses.extend(step.responses);
            response_request_cmd = Some(20104);
            resume_sync_word = Some(current_sync_word);
        } else if ctx.lock().await.battle_session_chosen.is_none() {
            // Keep the older flat capture path working when no session files
            // are available for this battle.
            legacy_resume = true;
            resume_sync_word = Some(current_sync_word);
        }
    }

    let mut packets = encode_group(&ctx, group.clone(), response_request_cmd).await?;
    if legacy_resume {
        let legacy_packets = handle_legacy_video_end(ctx.clone()).await?;
        if legacy_packets.is_empty() {
            resume_sync_word = None;
        } else {
            packets.extend(legacy_packets);
        }
    }
    if resume_sync_word.is_some() {
        ctx.lock().await.battle_auto_resume_sync_word = resume_sync_word;
    }

    info!(
        is_auto = request.is_auto,
        resumed_with_action = response_request_cmd == Some(20104) || legacy_resume,
        "Auto-battle requested"
    );
    Ok(packets)
}

fn skip_auto_resume_duplicate(connection: &mut ConnectionContext, sync_word: i32) -> bool {
    connection.battle_auto_resume_sync_word.take() == Some(sync_word)
}

/// Handle CS_BATTLE_VIDEO_END (20104): feed the next scripted action batch.
///
/// Batches are consumed in capture order. If the final action already delivered
/// the result but the client asks for another video-end before leaving the
/// battle, replay the cached terminal result once instead of leaving it waiting.
pub async fn handle_battle_video_end(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_VIDEO_END,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let duplicate_auto_resume = {
        let mut connection = ctx.lock().await;
        skip_auto_resume_duplicate(&mut connection, request.sync_word)
    };
    if duplicate_auto_resume {
        info!(
            sync_word = request.sync_word,
            "Ignoring video-end sync already answered during auto resume"
        );
        return Ok(Vec::new());
    }
    // A zero sync word is not an abandon signal (quit/skip have their own
    // commands); process it like any other video-end so a client quirk can
    // never end the battle prematurely.
    if request.sync_word == 0 {
        info!("Battle video ended without a sync word");
    }
    let terminal_result = {
        let mut connection = ctx.lock().await;
        if !connection.battle_active
            && connection.battle_result_served
            && !connection.battle_terminal_replay_served
        {
            if let Some(packet) = connection.battle_terminal_result.clone() {
                connection.battle_terminal_replay_served = true;
                Some(packet)
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some(packet) = terminal_result {
        info!(
            sync_word = request.sync_word,
            "Re-sending the terminal battle result for a late video-end"
        );
        return Ok(vec![packet]);
    }
    if !ctx.lock().await.battle_active {
        return Ok(Vec::new());
    }

    // A benched hero's turn encodes to zero packets once roster filtering
    // removes it; skip ahead to the next batch with visible actions instead of
    // stalling the match with an empty reply.
    loop {
        let Some(step) = take_step(&ctx, 20104).await else {
            break;
        };
        let packets = encode_step(&ctx, &step).await?;
        if !packets.is_empty() {
            return Ok(packets);
        }
        if !ctx.lock().await.battle_active {
            return Ok(packets);
        }
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

    // No recorded result at all (e.g. a result-less session slipped through):
    // synthesize a victory so the client can leave the battlefield instead of
    // hanging on a match that will never end.
    {
        let mut connection = ctx.lock().await;
        if connection.battle_session_chosen.is_some() {
            let round = connection.battle_round;
            let heroes = connection.battle_active_heroes.clone();
            connection.battle_result_served = true;
            connection.battle_active = false;
            connection.battle_pending_skills.clear();
            connection.battle_sync_advance_pending = false;
            connection.battle_auto_resume_sync_word = None;
            info!(round, "Battle steps exhausted without a recorded result; synthesizing victory");
            let result = SC_BATTLE_RESULT {
                result: 1,
                award: Vec::new(),
                detail_item_award: Vec::new(),
                player_exp: 0,
                hero_exp: 0,
                hero_relation: 0,
                args: Vec::new(),
                hero_id_list: heroes
                    .iter()
                    .map(|(hero_id, _, _)| crate::messages::pt_attr_int {
                        key: *hero_id as i16,
                        value: 0,
                    })
                    .collect(),
                round,
                statistic: Vec::new(),
                pos_effect: Vec::new(),
                is_replay: 0,
            };
            let packet = build_server_packet(20106, &result.encode())?;
            connection.battle_terminal_result = Some(packet.clone());
            connection.battle_terminal_replay_served = false;
            return Ok(vec![packet]);
        }
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

    encode_group(&ctx, group, Some(20104)).await
}

/// Build a direct skill acknowledgement. `result = 0` is used for taps that
/// cannot be paired with the next recorded action, rather than claiming they
/// succeeded and later attaching them to an unrelated hero.
fn skill_response(
    hero_id: i32,
    skill_id: i32,
    result: i8,
    rage: i16,
    sync_word: i32,
) -> Result<Vec<u8>, anyhow::Error> {
    build_server_packet(
        20115,
        &SC_BATTLE_USE_SKILL {
            hero_id,
            skill_id,
            result,
            skill_soul: 0,
            rage,
            sync_word,
        }
        .encode(),
    )
}

/// Handle CS_BATTLE_USE_SKILL (20108): replay only a matching captured skill,
/// or synthesize an acknowledgement when the next recorded player action is
/// exactly that hero/skill. Repeated or out-of-turn taps are rejected so they
/// cannot turn a later basic attack (or the victory action) into a ghost cast.
pub async fn handle_battle_use_skill(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_USE_SKILL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let (battle_active, result_served, hero_id, tid, sync_word) = {
        let connection = ctx.lock().await;
        let selected = connection
            .battle_active_heroes
            .iter()
            .find(|(_, hero_tid, _)| {
                request.skill_id == *hero_tid || request.skill_id / 100 == *hero_tid
            });
        let (hero_id, tid) = selected
            .map(|(hero_id, tid, _)| (*hero_id, *tid))
            .unwrap_or_default();
        (
            connection.battle_active,
            connection.battle_result_served,
            hero_id,
            tid,
            connection.battle_sync_word,
        )
    };

    if !battle_active {
        if result_served {
            info!(
                hero_id,
                skill_id = request.skill_id,
                sync_word,
                "Battle skill rejected after the result was served"
            );
            return Ok(vec![skill_response(hero_id, request.skill_id, 0, 0, sync_word)?]);
        }
        return Ok(Vec::new());
    }

    let Some(skill) = (hero_id != 0).then_some(BattlePendingSkill {
        hero_id,
        skill_id: request.skill_id,
    }) else {
        let mut connection = ctx.lock().await;
        if connection.battle_pending_skills.is_empty() {
            connection.battle_sync_advance_pending = true;
        }
        info!(
            skill_id = request.skill_id,
            sync_word,
            "Battle skill rejected: no deployed hero owns the skill"
        );
        return Ok(vec![skill_response(0, request.skill_id, 0, 0, sync_word)?]);
    };

    if ctx.lock().await.battle_pending_skills.contains(&skill) {
        info!(
            hero_id,
            tid,
            skill_id = request.skill_id,
            sync_word,
            "Repeated battle skill rejected while the same skill is pending"
        );
        return Ok(vec![skill_response(hero_id, request.skill_id, 0, 0, sync_word)?]);
    }

    if let Some(mut step) = take_matching_skill_step(&ctx, skill).await {
        let actor_map = {
            let mut connection = ctx.lock().await;
            connection.battle_sync_advance_pending = false;
            connection.battle_actor_map.clone()
        };
        let has_matching_ack = step.responses.iter().any(|response| {
            battle_skill_ack(response, &actor_map) == Some(skill)
        });
        let has_matching_action = step.responses.iter().any(|response| {
            battle_action_signature(response, &actor_map)
                == Some((1, skill.hero_id, skill.skill_id))
        });

        // Only preserve a captured ack after confirming it belongs to this
        // exact deployed hero and skill. Stale or unrelated acknowledgements
        // in a mixed group must not be queued onto a future action.
        step.responses.retain(|response| {
            response.cmd != 20115 || battle_skill_ack(response, &actor_map) == Some(skill)
        });
        for response in &mut step.responses {
            if battle_skill_ack(response, &actor_map) != Some(skill) {
                continue;
            }
            if let Some(object) = response.decoded.as_mut().and_then(Value::as_object_mut) {
                object.insert("hero_id".to_owned(), json!(skill.hero_id));
                object.insert("skill_id".to_owned(), json!(skill.skill_id));
                response.payload_hex = None;
            } else if let Some(mut raw) = response
                .payload_hex
                .as_deref()
                .and_then(decode_payload_hex)
            {
                if raw.len() >= 17 {
                    raw[..4].copy_from_slice(&skill.hero_id.to_be_bytes());
                    raw[4..8].copy_from_slice(&skill.skill_id.to_be_bytes());
                    response.decoded = Some(json!({
                        "hero_id": skill.hero_id,
                        "skill_id": skill.skill_id,
                        "result": raw[8] as i8,
                        "skill_soul": be_i16(&raw, 9).unwrap_or_default(),
                        "rage": be_i16(&raw, 11).unwrap_or_default(),
                        "sync_word": be_i32(&raw, 13).unwrap_or_default(),
                    }));
                    response.payload_hex = None;
                }
            }
        }

        // Keep this validated request pending until its exact action is served.
        // This also lets a captured ack arrive in a later 20104/20120 group
        // without trusting unrelated acknowledgements from the recording.
        if has_matching_action || has_matching_ack {
            let mut connection = ctx.lock().await;
            if !connection.battle_pending_skills.contains(&skill) {
                connection.battle_pending_skills.push_back(skill);
            }
        }
        let packets = encode_step(&ctx, &step).await?;
        if !packets.is_empty() {
            return Ok(packets);
        }
        info!(
            hero_id,
            tid,
            skill_id = request.skill_id,
            sync_word,
            "Matching battle skill response was filtered; acknowledging directly"
        );
        return Ok(vec![skill_response(hero_id, request.skill_id, 1, 10000, sync_word)?]);
    }

    let (matches_next_action, has_delayed_capture) = {
        let connection = ctx.lock().await;
        let matches_next_action = next_scripted_action_matches_skill(&connection, skill);
        let has_delayed_capture = !matches_next_action
            && captured_skill_ack_precedes_matching_action(&connection, skill);
        (matches_next_action, has_delayed_capture)
    };
    if !matches_next_action && has_delayed_capture {
        let mut connection = ctx.lock().await;
        if !connection.battle_pending_skills.contains(&skill) {
            connection.battle_pending_skills.push_back(skill);
        }
        connection.battle_sync_advance_pending = false;
        info!(
            hero_id,
            tid,
            skill_id = request.skill_id,
            sync_word,
            "Battle skill accepted for a matching captured action with a delayed acknowledgement"
        );
        // The capture sends this acknowledgement with its later action batch.
        return Ok(Vec::new());
    }
    if !matches_next_action {
        let mut connection = ctx.lock().await;
        // If a previous valid skill is already waiting for execution, its sync
        // poll will advance the action; otherwise let the next sync advance the
        // ordinary scripted action after this rejected tap.
        if connection.battle_pending_skills.is_empty() {
            connection.battle_sync_advance_pending = true;
        }
        info!(
            hero_id,
            tid,
            skill_id = request.skill_id,
            sync_word,
            "Battle skill rejected: it does not match the next player action"
        );
        return Ok(vec![skill_response(hero_id, request.skill_id, 0, 0, sync_word)?]);
    }

    {
        let mut connection = ctx.lock().await;
        if !connection.battle_pending_skills.contains(&skill) {
            connection.battle_pending_skills.push_back(skill);
        }
        connection.battle_sync_advance_pending = false;
    }
    info!(
        hero_id,
        tid,
        skill_id = request.skill_id,
        sync_word,
        "Battle skill acknowledged for matching scripted action"
    );
    Ok(vec![skill_response(hero_id, request.skill_id, 1, 10000, sync_word)?])
}

/// Handle CS_BATTLE_SYNC (20120): replay the recorded sync batch, if any.
pub async fn handle_battle_sync(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_SYNC,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    if let Some(step) = take_step(&ctx, 20120).await {
        let packets = encode_step(&ctx, &step).await?;
        if !packets.is_empty() {
            ctx.lock().await.battle_sync_advance_pending = false;
            return Ok(packets);
        }
    }
    // Awaiting skill execution: the client polls sync until its tap turns
    // into an action, and the official server answers the poll with the
    // action itself (session_3 step30). Serve the next action step instead
    // of an empty NONE the client just retries around until it gives up on
    // a loading screen.
    if !ctx.lock().await.battle_pending_skills.is_empty() {
        loop {
            let Some(step) = take_step(&ctx, 20104).await else {
                break;
            };
            let packets = encode_step(&ctx, &step).await?;
            if !packets.is_empty() {
                info!(
                    sync_word = request.sync_word,
                    "Battle sync served pending skill execution"
                );
                return Ok(packets);
            }
            if !ctx.lock().await.battle_active {
                return Ok(packets);
            }
        }
    }
    // A failed/out-of-turn skill still needs a normal action reply; otherwise
    // some clients keep polling sync even after receiving the failure ack.
    let rejected_skill_needs_advance = {
        let mut connection = ctx.lock().await;
        if connection.battle_active && connection.battle_sync_advance_pending {
            connection.battle_sync_advance_pending = false;
            true
        } else {
            false
        }
    };
    if rejected_skill_needs_advance {
        loop {
            let Some(step) = take_step(&ctx, 20104).await else {
                break;
            };
            let packets = encode_step(&ctx, &step).await?;
            if !packets.is_empty() {
                info!(
                    sync_word = request.sync_word,
                    "Battle sync advanced after a rejected skill"
                );
                return Ok(packets);
            }
            if !ctx.lock().await.battle_active {
                return Ok(packets);
            }
        }
    }
    // Otherwise re-send the last action once per sync word so the client can
    // confirm it is in step; further polls stay quiet until a newer action
    // is served (session_3 steps 30-31).
    {
        let mut connection = ctx.lock().await;
        if connection.battle_active && !connection.battle_result_served {
            if let Some((packet, sync)) = connection.battle_last_action.clone() {
                if connection.battle_last_repeat_sync != Some(sync) {
                    connection.battle_last_repeat_sync = Some(sync);
                    info!(
                        sync_word = request.sync_word,
                        repeated = sync,
                        "Battle sync repeated last action"
                    );
                    return Ok(vec![packet]);
                }
            }
        }
    }
    if ctx.lock().await.battle_active {
        let response = SC_BATTLE_NONE {};
        return Ok(vec![build_server_packet(20116, &response.encode())?]);
    }
    Ok(Vec::new())
}

/// Handle CS_BATTLE_FORCES_SKILL (20121): the emulator has no forces-skill
/// simulation, so acknowledge the tap with the current sync word instead of
/// leaving the client waiting for an energy update.
pub async fn handle_battle_forces_skill(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_FORCES_SKILL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let sync_word = ctx.lock().await.battle_sync_word;
    info!(
        skill_id = request.skill_id,
        sync_word, "Battle forces skill requested"
    );
    Ok(vec![build_server_packet(
        20122,
        &SC_BATTLE_FORCES_SKILL_ENERGY {
            energy: 0,
            sync_word,
        }
        .encode(),
    )?])
}

/// Handle CS_HERO_AUTO_RULE_CHANGE (20127): accept the new auto-battle rule
/// for the hero.
pub async fn handle_hero_auto_rule_change(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_HERO_AUTO_RULE_CHANGE,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    info!(
        hero_id = request.hero_id,
        rule = request.rule_type,
        "Hero auto rule changed"
    );
    Ok(vec![build_server_packet(
        20128,
        &SC_HERO_AUTO_RULE_CHANGE {
            hero_id: request.hero_id,
            rule_type: request.rule_type,
            result: 1,
        }
        .encode(),
    )?])
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
    let packet = build_server_packet(20106, &result.encode())?;
    {
        let mut connection = ctx.lock().await;
        connection.battle_terminal_result = Some(packet.clone());
        connection.battle_terminal_replay_served = false;
    }
    Ok(vec![packet])
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
            .filter(|hero| hero.pos > 0 && hero.hero_id != 0 && hero.tid != 0)
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

    // Reserve exact hero-id matches first so an earlier fallback match cannot
    // steal a selected hero from its own captured actor.
    for (old_id, old_tid, _) in recorded {
        if let Some((new_id, new_tid, _)) = deployed.iter().find(|hero| hero.0 == *old_id) {
            if assigned.insert(*new_id) {
                actor_map.insert(*old_id, (*new_id, *old_tid, *new_tid));
            }
        }
    }

    // Next preserve hero identity by tid, then pair remaining recorded actors
    // with remaining deployed slots. Excess captured actors stay unmapped and
    // are filtered from actions, orders and trigger responses.
    for (old_id, old_tid, _) in recorded {
        if actor_map.contains_key(old_id) {
            continue;
        }
        let target = deployed
            .iter()
            .find(|hero| hero.1 == *old_tid && !assigned.contains(&hero.0))
            .or_else(|| deployed.iter().find(|hero| !assigned.contains(&hero.0)));
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

/// Rebuild the attacker roster in an SC_BATTLE_FIELD_INFO raw payload. Captured
/// heroes outside the selected deployment are removed, and only one-to-one
/// actor mappings are retained for subsequent actions and result messages.
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

/// Attacker roster recorded in an SC_BATTLE_FIELD_INFO raw payload, as
/// (id, tid, slot). Used when the client never reported a formation (or
/// deployed nobody) so battle actions still resolve to valid actor ids.
fn recorded_attackers(raw: &[u8]) -> Option<Vec<(i32, i32, i8)>> {
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
    let mut attackers = Vec::with_capacity(recorded_count);
    for slot in 1..=recorded_count {
        let len = hero_entry_len(raw, offset)?;
        let id = be_i32(raw, offset)?;
        let tid = be_i32(raw, offset.checked_add(4)?)?;
        attackers.push((id, tid, slot as i8));
        offset = offset.checked_add(len)?;
    }
    Some(attackers)
}

fn recorded_roster(group: &TemplateGroup) -> Vec<(i32, i32, i8)> {
    group
        .responses
        .iter()
        .find(|response| response.cmd == 20101)
        .and_then(|response| {
            response
                .payload_hex
                .as_deref()
                .and_then(decode_payload_hex)
        })
        .and_then(|raw| recorded_attackers(&raw))
        .unwrap_or_default()
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

/// Keep SC_BATTLE_FIELD_INFO consistent with the client's requested stage when
/// this replay falls back to a recording captured on a different battlefield.
fn patch_enter_field_id(group: &mut TemplateGroup, requested_field_id: &str) {
    let Ok(field_id) = requested_field_id.parse::<i64>() else {
        return;
    };
    for response in group
        .responses
        .iter_mut()
        .filter(|response| response.cmd == 20101)
    {
        if let Some(mut raw) = response.payload_hex.as_deref().and_then(decode_payload_hex) {
            if let Some(field_id_bytes) = raw.get_mut(1..9) {
                field_id_bytes.copy_from_slice(&field_id.to_be_bytes());
                response.payload_hex = Some(to_hex(&raw));
            }
        }
        if let Some(object) = response.decoded.as_mut().and_then(Value::as_object_mut) {
            object.insert("battle_field_id".to_owned(), json!(field_id.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_resume_video_end_duplicate_is_ignored_once() {
        let mut connection = ConnectionContext::new("test".to_owned());
        connection.battle_auto_resume_sync_word = Some(71337005);

        assert!(skip_auto_resume_duplicate(&mut connection, 71337005));
        assert!(!skip_auto_resume_duplicate(&mut connection, 71337005));

        connection.battle_auto_resume_sync_word = Some(71337005);
        assert!(!skip_auto_resume_duplicate(&mut connection, 71337006));
        assert_eq!(connection.battle_auto_resume_sync_word, None);
    }

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
                    {"side": 1, "hero_id": 99, "effect_list": []},
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
        assert_eq!(remap_skill_id(1305, 1305, 1006), 1006);
        assert_eq!(decoded["target_id"], 43);
        assert_eq!(decoded["target_list"].as_array().unwrap().len(), 2);
        assert_eq!(decoded["target_list"][0]["hero_id"], 43);
        assert_eq!(decoded["target_list"][1]["side"], 2);
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
    fn battle_order_entries_follow_the_selected_roster_without_duplicates() {
        let actor_map = HashMap::from([
            (7, (42, 1305, 1006)),
            (8, (42, 1110, 1006)),
            (9, (43, 1006, 1206)),
        ]);
        let mut order = json!([
            {"key": 1, "value": 7},
            {"key": 1, "value": 8},
            {"key": 1, "value": 9},
            {"key": 1, "value": 99},
            {"key": 2, "value": 99}
        ]);

        remap_order_list(&mut order, &actor_map);
        assert_eq!(order, json!([
            {"key": 1, "value": 42},
            {"key": 1, "value": 43},
            {"key": 2, "value": 99}
        ]));
    }

    #[test]
    fn manual_skill_queue_waits_for_action_batches_not_sync_or_auto_toggles() {
        assert!(request_consumes_pending_skill(Some(20104)));
        assert!(request_consumes_pending_skill(Some(20108)));
        assert!(!request_consumes_pending_skill(Some(20120)));
        assert!(!request_consumes_pending_skill(Some(20113)));
        assert!(!request_consumes_pending_skill(None));
    }

    #[test]
    fn manual_skill_ack_uses_the_selected_actor_and_skill() {
        let actor_map = HashMap::from([(7, (42, 1305, 1006))]);
        let mut response = TemplateResponse {
            cmd: 20115,
            decoded: Some(json!({
                "hero_id": 7,
                "skill_id": 130501,
                "result": 1,
                "skill_soul": 0,
                "rage": 10000,
                "sync_word": 123
            })),
            payload_hex: Some("deadbeef".to_owned()),
        };

        remap_battle_response(&mut response, &actor_map);
        assert_eq!(response.decoded.as_ref().unwrap()["hero_id"], 42);
        assert_eq!(response.decoded.as_ref().unwrap()["skill_id"], 100601);
        assert!(response.payload_hex.is_none());
        assert_eq!(
            battle_skill_ack(&response, &actor_map),
            Some(BattlePendingSkill {
                hero_id: 42,
                skill_id: 100601
            })
        );

        let failed = TemplateResponse {
            cmd: 20115,
            decoded: Some(json!({
                "hero_id": 42,
                "skill_id": 100601,
                "result": 0
            })),
            payload_hex: None,
        };
        assert_eq!(battle_skill_ack(&failed, &actor_map), None);
    }

    #[test]
    fn order_change_trigger_uses_the_selected_hero() {
        let actor_map = HashMap::from([(7, (42, 1305, 1006))]);
        let mut response = TemplateResponse {
            cmd: 20129,
            decoded: Some(json!({
                "hero_side": 1,
                "hero_id": 7,
                "hero_tid": 1305,
                "hero_type": 1
            })),
            payload_hex: Some("deadbeef".to_owned()),
        };

        remap_battle_response(&mut response, &actor_map);
        assert_eq!(response.decoded.as_ref().unwrap()["hero_id"], 42);
        assert_eq!(response.decoded.as_ref().unwrap()["hero_tid"], 1006);
        assert!(response.payload_hex.is_none());
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
        let hero = |hero_id, tid, pos| crate::messages::pt_formation_hero_info {
            pos,
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
                formation_hero_list: vec![hero(7, 1110, 1)],
                assist_fight_list: Vec::new(),
                pet_id: 0,
            },
            crate::messages::pt_hero_formation {
                team_id: 1002,
                formation_id: 2,
                is_ready: 0,
                name: "selected team".to_owned(),
                formation_hero_list: vec![hero(42, 1006, 1), hero(99, 1206, 0)],
                assist_fight_list: Vec::new(),
                pet_id: 0,
            },
        ];
        assert_eq!(deployed_heroes(&formation, Some(1002)), vec![(42, 1006, 1)]);
    }

    #[test]
    fn actor_map_reserves_exact_matches_and_does_not_reuse_deployed_heroes() {
        let recorded = vec![
            (1, 1110, Vec::new()),
            (42, 1305, Vec::new()),
            (3, 1206, Vec::new()),
        ];
        let deployed = [(42, 1006, 1), (9, 1006, 2)];

        let actor_map = build_actor_map(&recorded, &deployed);
        assert_eq!(actor_map.get(&42), Some(&(42, 1305, 1006)));
        assert_eq!(actor_map.get(&1), Some(&(9, 1110, 1006)));
        assert!(!actor_map.contains_key(&3));
        assert_eq!(
            actor_map
                .values()
                .map(|mapped| mapped.0)
                .collect::<HashSet<_>>()
                .len(),
            2
        );
    }

    #[test]
    fn field_info_uses_the_requested_battlefield_on_capture_fallback() {
        let mut raw = field_info_with_two_attackers();
        raw[1..9].copy_from_slice(&1004i64.to_be_bytes());
        let mut group = TemplateGroup {
            responses: vec![TemplateResponse {
                cmd: 20101,
                decoded: Some(json!({"battle_field_id": "1004"})),
                payload_hex: Some(to_hex(&raw)),
            }],
        };

        patch_enter_field_id(&mut group, "1001");
        let patched = decode_payload_hex(group.responses[0].payload_hex.as_deref().unwrap())
            .unwrap();
        let decoded = crate::messages::SC_BATTLE_FIELD_INFO::decode(&patched);

        assert_eq!(decoded.battle_field_id, "1001");
        assert_eq!(
            group.responses[0].decoded.as_ref().unwrap()["battle_field_id"],
            "1001"
        );
    }

    #[test]
    fn field_info_removes_benched_capture_heroes() {
        let raw = field_info_with_two_attackers();
        let deployed = [(42, 1006, 1)];
        let (patched, roster) = patch_field_info_heroes(&raw, &deployed).unwrap();
        let decoded = crate::messages::SC_BATTLE_FIELD_INFO::decode(&patched);

        assert_eq!(decoded.battle_field_id, "1001");
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
        assert!(!roster.actor_map.contains_key(&8));
    }

    #[test]
    fn recorded_roster_lists_the_capture_attackers() {
        let raw = field_info_with_two_attackers();
        assert_eq!(
            recorded_attackers(&raw),
            Some(vec![(7, 1110, 1), (8, 1305, 2)])
        );
        assert_eq!(recorded_attackers(&[]), None);
        assert_eq!(recorded_attackers(&[1, 2, 3]), None);
    }

    #[test]
    fn recorded_battle_action_round_trips_byte_exact() {
        // Regression test: pt_battle_effect_info::encode used to write
        // count_list elements as strings while decode reads i64s, so every
        // re-encoded 20103 came out short and misaligned (session_1 step0:
        // 346 recorded bytes became 255) and the client stalled mid-battle.
        let session: Value =
            serde_json::from_str(include_str!("../../../data/battle/session_1.json"))
                .expect("session_1.json must parse");
        let hex = session["steps"][0]["responses"][0]["payload_hex"]
            .as_str()
            .expect("step0 must carry a raw payload");
        let raw = decode_payload_hex(hex).expect("payload_hex must decode");
        assert_eq!(raw.len(), 346);
        let message = crate::messages::SC_BATTLE_ACTION::decode(&raw);
        assert_eq!(message.hero_id, 2);
        assert_eq!(message.encode(), raw);
    }

    #[test]
    fn battle_result_args_encode_as_i64() {
        // SC_BATTLE_RESULT.args decodes Vec<i64>; the encoder must match or
        // the 20106 finale packet is misaligned.
        let message = crate::messages::SC_BATTLE_RESULT {
            result: 1,
            award: Vec::new(),
            detail_item_award: Vec::new(),
            player_exp: 0,
            hero_exp: 0,
            hero_relation: 0,
            args: vec!["7".to_owned()],
            hero_id_list: Vec::new(),
            round: 1,
            statistic: Vec::new(),
            pos_effect: Vec::new(),
            is_replay: 0,
        };
        let bytes = message.encode();
        assert_eq!(bytes.len(), 35);
        let back = crate::messages::SC_BATTLE_RESULT::decode(&bytes);
        assert_eq!(back.args, message.args);
    }

    #[test]
    fn attr_int_list_values_encode_as_i64() {
        // pt_attr_int_list.value feeds 20106 statistic entries; each element
        // is an i64 on the wire, not a length-prefixed string.
        let message = crate::messages::pt_attr_int_list {
            key: 3,
            value: vec!["11".to_owned(), "22".to_owned()],
        };
        let bytes = message.encode();
        assert_eq!(bytes.len(), 20);
        let mut reader = crate::packet::ProtocolByteBuf::new(&bytes);
        let back = crate::messages::pt_attr_int_list::decode(&mut reader);
        assert_eq!(back.key, 3);
        assert_eq!(back.value, message.value);
    }

    fn packet_cmd(packet: &[u8]) -> u32 {
        u32::from_be_bytes([packet[2], packet[3], packet[4], packet[5]])
    }

    #[tokio::test]
    async fn battle_sync_poll_serves_pending_skill_then_repeats_once() {
        // Regression test for the mid-battle loading stall: the client polls
        // 20120 until a tapped skill turns into an action (the official
        // server answers the poll with the action itself), then expects the
        // current action re-sent once as a sync confirmation. Answering NONE
        // to every poll made the client retry its tap and eventually give up.
        let mut connection = ConnectionContext::new("test".to_owned());
        let steps = load_session(0).map(|data| data.steps.len()).unwrap_or(13);
        connection.battle_active = true;
        connection.battle_session_chosen = Some(0);
        connection.battle_step_consumed = vec![false; steps];
        connection.battle_result_served = false;
        connection.battle_active_heroes = vec![(1, 1110, 1), (3, 1202, 2)];
        connection.battle_actor_map.insert(1, (1, 1110, 1110));
        connection.battle_actor_map.insert(2, (3, 1305, 1202));
        connection
            .battle_pending_skills
            .push_back(crate::state::BattlePendingSkill {
                hero_id: 3,
                skill_id: 120201,
            });
        let ctx = Arc::new(Mutex::new(connection));

        // First poll carries a pending skill: the next recorded action is the
        // same hero/skill, so it is served and the queue is consumed.
        let first = handle_battle_sync(ctx.clone(), CS_BATTLE_SYNC { sync_word: 0 })
            .await
            .expect("sync poll must serve the pending execution");
        assert!(!first.is_empty());
        assert_eq!(packet_cmd(&first[0]), 20103);
        assert!(ctx.lock().await.battle_pending_skills.is_empty());

        // Second poll has nothing pending: the last action is repeated once
        // so the client can confirm it is in step.
        let second = handle_battle_sync(ctx.clone(), CS_BATTLE_SYNC {
            sync_word: 41219004,
        })
        .await
        .expect("sync poll must repeat the last action");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0], first[0]);

        // Third poll: already repeated, so the server stays quiet with NONE.
        let third = handle_battle_sync(ctx.clone(), CS_BATTLE_SYNC {
            sync_word: 41219004,
        })
        .await
        .expect("sync poll must answer");
        assert_eq!(third.len(), 1);
        assert_eq!(packet_cmd(&third[0]), 20116);
    }

    #[tokio::test]
    async fn repeated_skill_tap_does_not_queue_multiple_casts() {
        let mut connection = ConnectionContext::new("test".to_owned());
        let steps = load_session(0).expect("session_1 must be available").steps.len();
        connection.battle_active = true;
        connection.battle_session_chosen = Some(0);
        connection.battle_step_consumed = vec![false; steps];
        connection.battle_result_served = false;
        connection.battle_active_heroes = vec![(1, 1110, 1), (3, 1202, 2)];
        connection.battle_actor_map.insert(1, (1, 1110, 1110));
        connection.battle_actor_map.insert(2, (3, 1305, 1202));
        let ctx = Arc::new(Mutex::new(connection));

        let first = handle_battle_use_skill(
            ctx.clone(),
            CS_BATTLE_USE_SKILL { skill_id: 120201 },
        )
        .await
        .expect("matching skill should be accepted");
        let (_, body) = crate::packet::parse_server_packet(&first[0], "").unwrap();
        assert_eq!(SC_BATTLE_USE_SKILL::decode(&body).result, 1);

        let duplicate = handle_battle_use_skill(
            ctx.clone(),
            CS_BATTLE_USE_SKILL { skill_id: 120201 },
        )
        .await
        .expect("duplicate tap should receive a rejection");
        let (_, body) = crate::packet::parse_server_packet(&duplicate[0], "").unwrap();
        assert_eq!(SC_BATTLE_USE_SKILL::decode(&body).result, 0);
        assert_eq!(ctx.lock().await.battle_pending_skills.len(), 1);

        let action = handle_battle_sync(
            ctx.clone(),
            CS_BATTLE_SYNC { sync_word: 41219003 },
        )
        .await
        .expect("one sync should execute the one pending skill");
        assert_eq!(packet_cmd(&action[0]), 20103);
        assert!(ctx.lock().await.battle_pending_skills.is_empty());
    }

    #[tokio::test]
    async fn mismatched_skill_is_rejected_and_sync_advances_without_rewriting_action() {
        let mut connection = ConnectionContext::new("test".to_owned());
        let steps = load_session(0).expect("session_1 must be available").steps.len();
        connection.battle_active = true;
        connection.battle_session_chosen = Some(0);
        connection.battle_step_consumed = vec![false; steps];
        connection.battle_result_served = false;
        connection.battle_active_heroes = vec![(1, 1110, 1), (3, 1202, 2)];
        connection.battle_actor_map.insert(1, (1, 1110, 1110));
        connection.battle_actor_map.insert(2, (3, 1305, 1202));
        let ctx = Arc::new(Mutex::new(connection));

        // The next recorded action is hero 3's 120201, not the requested
        // 120204. Do not claim that the latter is executing.
        let ack = handle_battle_use_skill(
            ctx.clone(),
            CS_BATTLE_USE_SKILL { skill_id: 120204 },
        )
        .await
        .expect("invalid skill tap should be acknowledged as rejected");
        assert_eq!(packet_cmd(&ack[0]), 20115);
        let (_, body) = crate::packet::parse_server_packet(&ack[0], "").unwrap();
        assert_eq!(SC_BATTLE_USE_SKILL::decode(&body).result, 0);
        assert!(ctx.lock().await.battle_sync_advance_pending);

        // A rejection still advances the expected script action on the next
        // sync, and the captured hero/skill/effects remain intact.
        let action_packets = handle_battle_sync(
            ctx.clone(),
            CS_BATTLE_SYNC { sync_word: 41219003 },
        )
        .await
        .expect("sync should advance after a rejected skill");
        assert_eq!(packet_cmd(&action_packets[0]), 20103);
        let (_, body) = crate::packet::parse_server_packet(&action_packets[0], "").unwrap();
        let action = crate::messages::SC_BATTLE_ACTION::decode(&body);
        assert_eq!(action.hero_id, 3);
        assert_eq!(action.skill_id, 120201);
        assert!(!ctx.lock().await.battle_sync_advance_pending);
    }

    #[tokio::test]
    async fn delayed_skill_ack_is_replayed_only_for_a_matching_pending_action() {
        let session = load_session(2).expect("session_3 must be available");
        let mut auto_connection = ConnectionContext::new("auto".to_owned());
        auto_connection.battle_active = true;
        auto_connection.battle_session_chosen = Some(2);
        auto_connection.battle_step_consumed = vec![false; session.steps.len()];
        auto_connection.battle_active_heroes = vec![(1, 1110, 1), (3, 1202, 2)];
        auto_connection.battle_actor_map.insert(1, (1, 1110, 1110));
        auto_connection.battle_actor_map.insert(2, (3, 1305, 1202));
        let auto_ctx = Arc::new(Mutex::new(auto_connection));

        // Session 3 records two delayed acknowledgements in step 29 for a
        // manual tap. An auto-battle replay with no matching tap must omit them.
        let auto_packets = encode_step(&auto_ctx, &session.steps[29])
            .await
            .expect("auto action step should encode");
        assert_eq!(
            auto_packets
                .iter()
                .filter(|packet| packet_cmd(packet) == 20115)
                .count(),
            0
        );

        let mut connection = ConnectionContext::new("manual".to_owned());
        connection.battle_active = true;
        connection.battle_session_chosen = Some(2);
        connection.battle_step_consumed = vec![false; session.steps.len()];
        connection.battle_step_consumed[..28].fill(true);
        connection.battle_active_heroes = vec![(1, 1110, 1), (3, 1202, 2)];
        connection.battle_actor_map.insert(1, (1, 1110, 1110));
        connection.battle_actor_map.insert(2, (3, 1305, 1202));
        let ctx = Arc::new(Mutex::new(connection));

        // Step 28 is an empty 20108; its matching ack arrives in step 29,
        // before the corresponding 111004 player action at step 32.
        let delayed = handle_battle_use_skill(
            ctx.clone(),
            CS_BATTLE_USE_SKILL { skill_id: 111004 },
        )
        .await
        .expect("matching delayed skill should be queued");
        assert!(delayed.is_empty());
        assert_eq!(ctx.lock().await.battle_pending_skills.len(), 1);

        let ack_batch = handle_battle_video_end(
            ctx.clone(),
            CS_BATTLE_VIDEO_END { sync_word: 0 },
        )
        .await
        .expect("later capture batch should deliver its skill ack");
        assert_eq!(
            ack_batch
                .iter()
                .filter(|packet| packet_cmd(packet) == 20115)
                .count(),
            1
        );
        assert_eq!(ctx.lock().await.battle_pending_skills.len(), 1);

        // The current 111001 action does not consume the queued 111004 skill;
        // the next matching action does, with its captured effects unchanged.
        let matching_action = handle_battle_video_end(
            ctx.clone(),
            CS_BATTLE_VIDEO_END { sync_word: 0 },
        )
        .await
        .expect("matching future action should be served");
        let action_packet = matching_action
            .iter()
            .find(|packet| packet_cmd(packet) == 20103)
            .expect("matching action should be present");
        let (_, body) = crate::packet::parse_server_packet(action_packet, "").unwrap();
        let action = crate::messages::SC_BATTLE_ACTION::decode(&body);
        assert_eq!(action.hero_id, 1);
        assert_eq!(action.skill_id, 111004);
        assert!(ctx.lock().await.battle_pending_skills.is_empty());
    }

    #[tokio::test]
    async fn pending_skill_never_rewrites_final_action_and_is_cleared_on_result() {
        let session = load_session(0).expect("session_1 must be available");
        let mut connection = ConnectionContext::new("test".to_owned());
        connection.battle_active = true;
        connection.battle_session_chosen = Some(0);
        connection.battle_step_consumed = vec![false; session.steps.len()];
        connection.battle_result_served = false;
        connection.battle_active_heroes = vec![(1, 1110, 1), (3, 1202, 2)];
        connection.battle_actor_map.insert(1, (1, 1110, 1110));
        connection.battle_actor_map.insert(2, (3, 1305, 1202));
        connection
            .battle_pending_skills
            .push_back(BattlePendingSkill {
                hero_id: 3,
                skill_id: 120204,
            });
        let ctx = Arc::new(Mutex::new(connection));
        let final_step = session
            .steps
            .iter()
            .find(|step| step.contains(20106))
            .expect("session must contain its victory result");

        let packets = encode_step(&ctx, final_step)
            .await
            .expect("final step should encode");
        let action_packet = packets
            .iter()
            .find(|packet| packet_cmd(packet) == 20103)
            .expect("final step should contain its action");
        let (_, body) = crate::packet::parse_server_packet(action_packet, "").unwrap();
        let action = crate::messages::SC_BATTLE_ACTION::decode(&body);
        assert_eq!(action.hero_id, 1);
        assert_eq!(action.skill_id, 111001);
        assert!(packets.iter().any(|packet| packet_cmd(packet) == 20106));

        let connection = ctx.lock().await;
        assert!(connection.battle_pending_skills.is_empty());
        assert!(!connection.battle_active);
        assert!(connection.battle_result_served);
        assert!(connection.battle_terminal_result.is_some());
    }

    #[tokio::test]
    async fn post_result_skill_is_rejected_and_late_video_end_replays_result_once() {
        let terminal_result = build_server_packet(
            20106,
            &retreat_result(2).encode(),
        )
        .unwrap();
        let mut connection = ConnectionContext::new("test".to_owned());
        connection.battle_active = false;
        connection.battle_result_served = true;
        connection.battle_active_heroes = vec![(3, 1202, 2)];
        connection.battle_sync_word = 41219022;
        connection.battle_terminal_result = Some(terminal_result.clone());
        let ctx = Arc::new(Mutex::new(connection));

        let skill_ack = handle_battle_use_skill(
            ctx.clone(),
            CS_BATTLE_USE_SKILL { skill_id: 120201 },
        )
        .await
        .expect("post-result skill tap should not be dropped");
        assert_eq!(packet_cmd(&skill_ack[0]), 20115);
        let (_, body) = crate::packet::parse_server_packet(&skill_ack[0], "").unwrap();
        assert_eq!(SC_BATTLE_USE_SKILL::decode(&body).result, 0);

        let replay = handle_battle_video_end(
            ctx.clone(),
            CS_BATTLE_VIDEO_END { sync_word: 0 },
        )
        .await
        .expect("late video-end should receive a terminal result");
        assert_eq!(replay, vec![terminal_result]);
        assert!(ctx.lock().await.battle_terminal_replay_served);

        let duplicate = handle_battle_video_end(
            ctx,
            CS_BATTLE_VIDEO_END { sync_word: 0 },
        )
        .await
        .expect("duplicate late video-end should be harmless");
        assert!(duplicate.is_empty());
    }
}
