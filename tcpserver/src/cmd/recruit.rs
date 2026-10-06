//! Gacha/recruit flows replayed from recorded captures.
//!
//! `CS_RECRUIT_ITEM` (item gacha), `CS_RECRUIT_HERO_NEW_PREPARE` /
//! `CS_RECRUIT_HERO_NEW_CONFIRM` (hero gacha) and the saved-pull list all
//! have capture-specific responses (pity counters, pulled heroes, bag and
//! attribute updates), so they are replayed byte-exact from `recruit/`.

use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::info;

use crate::{
    cmd::battle::absorb_attr_updates,
    messages::{
        CS_RECRUIT_HERO_NEW_CONFIRM, CS_RECRUIT_HERO_NEW_PREPARE, CS_RECRUIT_HERO_NEW_SAVE_LIST,
        CS_RECRUIT_ITEM, SC_RECRUIT_HERO_NEW_SAVE_LIST,
    },
    packet::build_server_packet,
    sequence::{TemplateFile, TemplateGroup, TemplateResponse},
    state::ConnectionContext,
};

const ITEM_PULL_DATA: &str = "recruit/item_pull.json";
const HERO_PREPARE_DATA: &str = "recruit/hero_new_prepare.json";
const HERO_CONFIRM_DATA: &str = "recruit/hero_new_confirm.json";
const HERO_SAVE_LIST_DATA: &str = "recruit/hero_new_save_list.json";

/// One recorded item-gacha pull with the request that produced it.
#[derive(Clone, Debug, Default, Deserialize)]
struct PullRequest {
    #[serde(default)]
    id: Option<i16>,
    #[serde(default)]
    times: Option<i8>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PullEntry {
    #[serde(default)]
    request: Option<PullRequest>,
    #[serde(default)]
    responses: Vec<TemplateResponse>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PullFile {
    #[serde(default)]
    groups: Vec<PullEntry>,
}

async fn encode_group(
    ctx: &Arc<Mutex<ConnectionContext>>,
    group: &TemplateGroup,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let cursor = ctx.lock().await.replay_cursor.clone();
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, group);
    }
    Ok(packets)
}

/// Handle CS_RECRUIT_ITEM (13051): replay the recorded pull batch.
///
/// The recorded pull matching the requested pool and pull count wins; any
/// recorded pull is the fallback so repeated gacha keeps working.
pub async fn handle_recruit_item(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_RECRUIT_ITEM,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let file: PullFile = crate::data_loader::GameDataLoader::load_struct(ITEM_PULL_DATA)?;
    let matched = file
        .groups
        .iter()
        .find(|entry| {
            entry
                .request
                .as_ref()
                .map(|pull| pull.id == Some(request.id) && pull.times == Some(request.times))
                .unwrap_or(false)
        })
        .or_else(|| file.groups.first());
    let Some(entry) = matched else {
        return Ok(Vec::new());
    };
    info!(id = request.id, times = request.times, "Item gacha pull replayed");
    let group = TemplateGroup {
        responses: entry.responses.clone(),
    };
    encode_group(&ctx, &group).await
}

/// Handle CS_RECRUIT_HERO_NEW_PREPARE (13292): consume the recorded prepares
/// in order; the last recorded prepare is replayed once exhausted.
pub async fn handle_recruit_hero_new_prepare(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_RECRUIT_HERO_NEW_PREPARE,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(HERO_PREPARE_DATA)?;
    let group = {
        let mut connection = ctx.lock().await;
        let index = connection.recruit_prepare_index.min(script.group_count().saturating_sub(1));
        if connection.recruit_prepare_index < script.group_count() {
            connection.recruit_prepare_index += 1;
        }
        script.group(index).cloned().unwrap_or_default()
    };
    info!("Hero gacha prepare replayed");
    encode_group(&ctx, &group).await
}

/// Handle CS_RECRUIT_HERO_NEW_CONFIRM (13294): replay the recorded confirm
/// batch (new hero, shards and updates).
pub async fn handle_recruit_hero_new_confirm(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_RECRUIT_HERO_NEW_CONFIRM,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(HERO_CONFIRM_DATA)?;
    let Some(group) = script.first_group().cloned() else {
        return Ok(Vec::new());
    };
    info!("Hero gacha confirm replayed");
    encode_group(&ctx, &group).await
}

/// Handle CS_RECRUIT_HERO_NEW_SAVE_LIST (13290): replay the recorded
/// save-list responses in order; once exhausted the last recording is
/// replayed.  Without capture data an empty list is reported.
pub async fn handle_recruit_hero_new_save_list(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_RECRUIT_HERO_NEW_SAVE_LIST,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let Ok(script) = TemplateFile::load(HERO_SAVE_LIST_DATA) else {
        return Ok(vec![build_server_packet(
            13291,
            &SC_RECRUIT_HERO_NEW_SAVE_LIST {
                item_list: Vec::new(),
            }
            .encode(),
        )?]);
    };
    if script.group_count() == 0 {
        return Ok(Vec::new());
    }
    let group = {
        let mut connection = ctx.lock().await;
        let index = connection.recruit_save_index.min(script.group_count() - 1);
        if connection.recruit_save_index < script.group_count() {
            connection.recruit_save_index += 1;
        }
        script.group(index).cloned().unwrap_or_default()
    };
    encode_group(&ctx, &group).await
}
