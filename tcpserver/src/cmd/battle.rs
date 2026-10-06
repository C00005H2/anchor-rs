//! Battle flow driven by recorded battle sessions.
//!
//! Each `CS_BATTLE_FIELD_ENTER` in the capture starts a *session* stored under
//! `battle/session_{n}.json` with the entry sequence, the auto-battle push and
//! every action batch (`CS_BATTLE_VIDEO_END`, `CS_BATTLE_USE_SKILL`,
//! `CS_BATTLE_SYNC`) in capture order.  The final batch of a session carries
//! `SC_BATTLE_RESULT` plus all reward updates (XP, items, level-ups), so
//! battles replay their rewards byte-exact.  Attribute updates inside the
//! script are absorbed into the local profile so later claims stay coherent.
//!
//! When no session files exist the legacy flat `battle/video_end.json` queue
//! is used instead.

use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::info;

use crate::{
    capture_replay::decode_payload_hex,
    data_loader::GameDataLoader,
    messages::{
        CS_BATTLE_AUTO, CS_BATTLE_FIELD_ENTER, CS_BATTLE_START, CS_BATTLE_SYNC,
        CS_BATTLE_USE_SKILL, CS_BATTLE_VIDEO_END, SC_BATTLE_USE_SKILL,
    },
    packet::build_server_packet,
    sequence::{TemplateFile, TemplateGroup, TemplateResponse},
    state::ConnectionContext,
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
        if matches!(response.cmd, 20103 | 20105 | 20114 | 20115 | 20125) {
            if let Some(sync) = response
                .decoded
                .as_ref()
                .and_then(|decoded| decoded.get("sync_word"))
                .and_then(|sync| sync.as_i64())
            {
                connection.battle_sync_word = sync as i32;
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

/// Send one consumed step and absorb its attribute updates.
async fn encode_step(
    ctx: &Arc<Mutex<ConnectionContext>>,
    step: &BattleStep,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let group = step.as_group();
    let cursor = ctx.lock().await.replay_cursor.clone();
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, &group);
    }
    Ok(packets)
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

    let (cursor, formation, mut group) = {
        let mut connection = ctx.lock().await;
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
        connection.battle_script_index = 0;
        info!(
            battle_type = request.battle_type,
            battle_field_id = %request.battle_field_id,
            session = index + 1,
            steps = session.steps.len(),
            "Battle field entered (session)"
        );
        let group = session.enter.clone().unwrap_or_default();
        (connection.replay_cursor.clone(), connection.formation.clone(), group)
    };

    if !formation.is_empty() {
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
        patch_enter_formation(&mut group, &deployed_heroes(&formation));
    }

    group.encode(&cursor)
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

    let (cursor, formation) = {
        let mut connection = ctx.lock().await;
        connection.battle_active = true;
        connection.battle_script_index = 0;
        connection.battle_session_chosen = None;
        connection.battle_auto_served = false;
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
                    // The patched formation must win over the recorded raw
                    // bytes, otherwise freshly recruited heroes never show up
                    // on the deploy screen.
                    response.payload_hex = None;
                }
            }
        }
        patch_enter_formation(&mut group, &deployed_heroes(&formation));
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
            }
        }
    }

    let cursor = ctx.lock().await.replay_cursor.clone();
    info!(is_auto = request.is_auto, "Auto-battle requested");
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, group);
    }
    Ok(packets)
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
        let mut connection = ctx.lock().await;
        connection.battle_active = false;
        connection.battle_script_index = 0;
        info!("Battle finished");
        return Ok(Vec::new());
    }

    if let Some(step) = take_step(&ctx, 20104).await {
        return encode_step(&ctx, &step).await;
    }

    // Out of recorded video-end batches: re-serve the result batch so the
    // client still receives rewards when step counts diverge.
    if let Some(reward) = {
        let connection = ctx.lock().await;
        connection
            .battle_session_chosen
            .and_then(load_session)
            .and_then(|session| session.steps.iter().rev().find(|step| step.contains(20106)).cloned())
    } {
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
    let (cursor, group) = {
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

/// Handle CS_BATTLE_USE_SKILL (20108): replay the recorded skill batch.
///
/// Manual battles interleave skill batches with action batches; each recorded
/// batch is consumed once, in capture order.  Outside a recorded manual
/// battle the real server replied with nothing.
pub async fn handle_battle_use_skill(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_BATTLE_USE_SKILL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    if let Some(step) = take_step(&ctx, 20108).await {
        return encode_step(&ctx, &step).await;
    }
    // Manual battle outside a recorded manual session: acknowledge the skill
    // so the client does not stall waiting for SC_BATTLE_USE_SKILL.
    let sync_word = ctx.lock().await.battle_sync_word;
    info!(
        skill_id = request.skill_id,
        sync_word = sync_word,
        "Battle skill acknowledged (no recorded batch)"
    );
    Ok(vec![build_server_packet(
        20115,
        &SC_BATTLE_USE_SKILL {
            hero_id: 0,
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
    Ok(Vec::new())
}

/// Handle CS_BATTLE_QUIT (20107): the client abandons the current battle.
///
/// The capture recorded no reply for it, so just clear local battle state;
/// the next field enter re-initialises step consumption.
pub async fn handle_battle_quit(
    ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    {
        let mut connection = ctx.lock().await;
        connection.battle_active = false;
    }
    info!("Battle quit");
    Ok(Vec::new())
}

/// Heroes the client actually deployed, in formation order, as
/// (id, tid, formation slot).
///
/// Prefers the ready-marked team, then team 1001, then the first team with
/// any heroes.
fn deployed_heroes(formation: &[crate::messages::pt_hero_formation]) -> Vec<(i32, i32, i8)> {
    let team = formation
        .iter()
        .find(|team| team.is_ready == 1 && !team.formation_hero_list.is_empty())
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

fn hero_pos(bytes: &[u8]) -> (i16, i16) {
    (
        be_i16(bytes, 9).unwrap_or_default(),
        be_i16(bytes, 11).unwrap_or_default(),
    )
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
fn hero_entry_len(raw: &[u8], start: usize) -> Option<usize> {
    let skills = be_i16(raw, start + 40)? as usize;
    Some(54 + 5 * skills)
}

fn skip_assist_list(raw: &[u8], offset: usize) -> Option<usize> {
    let count = be_i16(raw, offset)? as usize;
    let mut offset = offset + 2;
    for _ in 0..count {
        // hero_tid i32 + hero_lv i16 + hero_evolution i16 + skill count i16.
        offset += 4 + 2 + 2;
        let skills = be_i16(raw, offset)? as usize;
        offset += 2 + 4 * skills;
    }
    Some(offset)
}

fn patch_field_info_heroes(raw: &[u8], deployed: &[(i32, i32, i8)]) -> Option<Vec<u8>> {
    // battle_type i8 + field id i64 + player id i64, then the name string.
    let mut offset = 1 + 8 + 8;
    let name_len = be_i16(raw, offset)? as usize & 0x7fff;
    // name (len+i16) + player_lv i16 + total_hp i64 + avatar i16 + count i16.
    let count_off = offset + 2 + name_len + 2 + 8 + 2;
    let count = be_i16(raw, count_off)? as usize;
    offset = count_off + 2;

    let mut recorded: Vec<(i32, i32, Vec<u8>)> = Vec::new();
    for _ in 0..count {
        let start = offset;
        let len = hero_entry_len(raw, start)?;
        let end = start + len;
        recorded.push((
            be_i32(raw, start)?,
            be_i32(raw, start + 4)?,
            raw.get(start..end)?.to_vec(),
        ));
        offset = end;
    }
    let mid_start = offset;

    // Attacker qte energy + assist list, then the full defender block.
    offset += 2;
    offset = skip_assist_list(raw, offset)?;
    offset += 8; // defender player id
    let dname = be_i16(raw, offset)? as usize & 0x7fff;
    offset += 2 + dname + 2 + 8 + 2;
    let dcount = be_i16(raw, offset)? as usize;
    offset += 2;
    for _ in 0..dcount {
        offset += hero_entry_len(raw, offset)?;
    }
    offset += 2;
    offset = skip_assist_list(raw, offset)?;

    // hero_order: (side i16, hero id i32) entries; appended for added heroes.
    let order_off = offset;
    let order_count = be_i16(raw, order_off)? as usize;
    let order_start = order_off + 2;
    let order_end = order_start + 6 * order_count;
    let tail = raw.get(order_end..)?;

    // Unchanged lineup: keep the recorded payload untouched.
    if deployed.len() == recorded.len()
        && deployed
            .iter()
            .all(|(id, _, _)| recorded.iter().any(|(rid, _, _)| rid == id))
    {
        return None;
    }

    let mut used: Vec<(i16, i16)> = recorded.iter().map(|entry| hero_pos(&entry.2)).collect();

    let mut entries: Vec<Vec<u8>> = Vec::new();
    let mut added: Vec<i32> = Vec::new();
    for (id, tid, slot) in deployed {
        match recorded.iter().find(|entry| entry.0 == *id) {
            Some(entry) => {
                let mut bytes = entry.2.clone();
                bytes[4..8].copy_from_slice(&tid.to_be_bytes());
                entries.push(bytes);
            }
            None => {
                let cell = slot_cell(*slot, &used);
                used.push(cell);
                added.push(*id);
                entries.push(synth_hero_entry(*id, *tid, cell));
            }
        }
    }
    // Benched recorded heroes stay in the lineup so recorded action batches
    // never reference an unknown hero id.
    for entry in &recorded {
        if !deployed.iter().any(|(id, _, _)| *id == entry.0) {
            entries.push(entry.2.clone());
        }
    }

    let mut out = Vec::with_capacity(raw.len() + 96);
    out.extend_from_slice(&raw[..count_off]);
    out.extend_from_slice(&(entries.len() as i16).to_be_bytes());
    for entry in &entries {
        out.extend_from_slice(entry);
    }
    out.extend_from_slice(&raw[mid_start..order_off]);
    out.extend_from_slice(&((order_count + added.len()) as i16).to_be_bytes());
    out.extend_from_slice(&raw[order_start..order_end]);
    for id in &added {
        out.extend_from_slice(&1i16.to_be_bytes());
        out.extend_from_slice(&id.to_be_bytes());
    }
    out.extend_from_slice(tail);
    Some(out)
}

/// Apply the client's deployed formation to a battle entry group's 20101.
fn patch_enter_formation(group: &mut TemplateGroup, deployed: &[(i32, i32, i8)]) {
    if deployed.is_empty() {
        return;
    }
    for response in group.responses.iter_mut() {
        if response.cmd != 20101 {
            continue;
        }
        let Some(raw) = response.payload_hex.as_deref().and_then(decode_payload_hex) else {
            continue;
        };
        match patch_field_info_heroes(&raw, deployed) {
            Some(patched) => {
                info!(
                    deployed = ?deployed,
                    bytes = patched.len(),
                    "Battle field info patched with client formation"
                );
                response.payload_hex = Some(to_hex(&patched));
            }
            // None = lineup unchanged (or unparsable payload); the recorded
            // bytes are already correct in that case.
            None => {}
        }
    }
}
