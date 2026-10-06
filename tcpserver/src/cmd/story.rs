//! Main-story flows: stage completion and the "skip duplicate story" pass.

use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::info;

use crate::{
    cmd::battle::absorb_attr_updates,
    data_loader::GameDataLoader,
    messages::{
        CS_DUP_ONLY_STORY_PASS, CS_MAIN_STORY_PLAY_CHAPTER_PIC, CS_MAIN_STORY_STAGE_AWARD,
        CS_STORY_OVER, SC_MAIN_STORY_INFO, SC_MAIN_STORY_PLAY_CHAPTER_PIC,
        SC_MAIN_STORY_STAGE_AWARD_LIST, SC_PROP_AWARD_SEND, pt_prop_award,
    },
    packet::build_server_packet,
    sequence::{TemplateGroup, TemplateResponse},
    state::ConnectionContext,
};

const STORY_PASS_DATA: &str = "story/dup_only_story_pass.json";

#[derive(Clone, Debug, Default, Deserialize)]
struct StoryPassRequest {
    #[serde(default)]
    battle_type: Option<i32>,
    #[serde(default)]
    field_id: Option<i32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct StoryPassEntry {
    #[serde(default)]
    request: Option<StoryPassRequest>,
    #[serde(default)]
    responses: Vec<TemplateResponse>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct StoryPassFile {
    #[serde(default)]
    groups: Vec<StoryPassEntry>,
}

/// Handle CS_STORY_OVER (12054): the real server sent no reply.
pub async fn handle_story_over(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_STORY_OVER,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    info!("Story dialogue finished");
    Ok(Vec::new())
}

/// Handle CS_MAIN_STORY_PLAY_CHAPTER_PIC (18051): acknowledge the chapter
/// splash image so the client keeps flowing into stage select.
pub async fn handle_main_story_play_chapter_pic(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_MAIN_STORY_PLAY_CHAPTER_PIC,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    info!(
        chapter_id = request.chapter_id,
        "Main story chapter pic played"
    );
    Ok(vec![build_server_packet(
        18052,
        &SC_MAIN_STORY_PLAY_CHAPTER_PIC {
            result: 1,
            chapter_id: request.chapter_id,
        }
        .encode(),
    )?])
}

/// Handle CS_DUP_ONLY_STORY_PASS (18012): replay the recorded skip-pass flow
/// (story/manual updates, unread notices, bag and attribute rewards).
///
/// Every recorded pass is consumed once, preferring the recording for the
/// requested stage; the last recording stays sticky afterwards so repeated
/// story stages keep getting a full progression push.
pub async fn handle_dup_only_story_pass(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_DUP_ONLY_STORY_PASS,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let file: StoryPassFile = GameDataLoader::load_struct(STORY_PASS_DATA)?;
    if file.groups.is_empty() {
        return Ok(Vec::new());
    }

    let group = {
        let mut connection = ctx.lock().await;
        let mut pick = None;
        for (index, entry) in file.groups.iter().enumerate() {
            if connection.story_pass_taken.contains(&index) {
                continue;
            }
            if entry.request.as_ref().and_then(|req| req.field_id) == Some(request.field_id) {
                pick = Some(index);
                break;
            }
            if pick.is_none() {
                pick = Some(index);
            }
        }
        let index = pick.unwrap_or(file.groups.len() - 1);
        if !connection.story_pass_taken.contains(&index) {
            connection.story_pass_taken.push(index);
        }
        TemplateGroup {
            responses: file.groups[index].responses.clone(),
        }
    };

    let cursor = ctx.lock().await.replay_cursor.clone();
    info!(
        battle_type = request.battle_type,
        field_id = request.field_id,
        "Story stage skipped (scripted replay)"
    );
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, &group);
    }
    Ok(packets)
}


/// Handle CS_MAIN_STORY_STAGE_AWARD (18053): claim the first-clear reward of a
/// main-story stage.  The claim is remembered per connection and confirmed
/// with the updated award list plus a small gold grant, so the stage no
/// longer reverts to an uncompleted state on the client.
pub async fn handle_main_story_stage_award(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_MAIN_STORY_STAGE_AWARD,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let stage_list = {
        let mut connection = ctx.lock().await;
        if !connection.story_award_claimed.contains(&request.stage_id) {
            connection.story_award_claimed.push(request.stage_id);
        }
        connection.story_award_claimed.clone()
    };

    info!(stage_id = request.stage_id, "Main story stage award claimed");
    Ok(vec![
        build_server_packet(
            18054,
            &SC_MAIN_STORY_STAGE_AWARD_LIST {
                stage_list: stage_list.clone(),
            }
            .encode(),
        )?,
        build_server_packet(
            17013,
            &SC_PROP_AWARD_SEND {
                award_list: vec![pt_prop_award {
                    tid: 1,
                    count: 6000,
                    color: 2,
                    expiredOddTime: 0,
                    expiredTime: 0,
                    refine_lv: 0,
                    total_attr: Vec::new(),
                    break_add_attr: Vec::new(),
                    skill_effect: Vec::new(),
                }],
            }
            .encode(),
        )?,
    ])
}

/// Rewrite an outgoing `SC_MAIN_STORY_INFO` (18000) packet so every story
/// stage is unlocked and marked cleared, letting the player enter any map
/// without grinding the campaign on the real account.
///
/// Stage ids follow the captured `chapter * 1000 + index` scheme.  Unknown
/// ids are ignored by the client, so a generous range is safe.
pub fn maybe_expand_story_unlock(pkt: Vec<u8>) -> Vec<u8> {
    if pkt.len() < 6 {
        return pkt;
    }
    let cmd = u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]);
    if cmd != 18000 {
        return pkt;
    }
    let body = pkt[6..].to_vec();
    let mut info = SC_MAIN_STORY_INFO::decode(&body);

    let mut stages: Vec<i32> = Vec::new();
    let mut chapters: Vec<i16> = Vec::new();
    for chapter in 1..=10i32 {
        chapters.push(chapter as i16);
        for index in 1..=10i32 {
            stages.push(chapter * 1000 + index);
        }
    }
    for stage in &stages {
        if !info.now_stage_list.contains(stage) {
            info.now_stage_list.push(*stage);
        }
        if !info.pass_stage_list.contains(stage) {
            info.pass_stage_list.push(*stage);
        }
    }
    for chapter in chapters {
        if !info.play_chapter_pic_list.contains(&chapter) {
            info.play_chapter_pic_list.push(chapter);
        }
    }
    info.ongoing_stage_id = 0;

    match build_server_packet(18000, &info.encode()) {
        Ok(expanded) => expanded,
        Err(_) => pkt,
    }
}
