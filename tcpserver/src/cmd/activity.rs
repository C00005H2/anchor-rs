//! Reward-claim flows driven by the `progression/` data tables extracted from
//! a capture.  Claims are once per id, rewards update the local player
//! profile, and panels always reflect the current claim state.

use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::info;

use crate::{
    data_loader::GameDataLoader,
    messages::{
        CS_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE, CS_DAILY_SIGN, CS_GAIN_ACHIEVEMENT_AWARD,
        CS_GAIN_ALL_FUND, CS_GAIN_OPEN_SERVER_SIGN_REWARD, CS_GAIN_SEVEN_DAY_REWARD,
        CS_NOVICE_TRAINING_PANEL,
        CS_NOVICE_TRAINING_RECEIVE_TASK, SC_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE,
        SC_GAIN_ACHIEVEMENT_AWARD, SC_NOVICE_TRAINING_RECEIVE_TASK,
        SC_OPEN_SERVER_SIGN_PANEL_INFO, SC_SEVEN_DAY_PANEL_INFO, SC_UPDATE_ACHIEVEMENT_INFO,
        SC_UPDATE_COMPLETE_ACHIEVE_INFO,
    },
    packet::build_server_packet,
    progression::{
        grant_rewards, unread_packets, AchievementTable, DayReward, IdReward, NoviceTrainingTable,
        SignTable,
    },
    sequence::TemplateFile,
    state::ConnectionContext,
};

const ACHIEVEMENT_DATA: &str = "progression/achievement.json";
const SEVEN_DAY_DATA: &str = "progression/seven_day.json";
const OPEN_SERVER_SIGN_DATA: &str = "progression/open_server_sign.json";
const NOVICE_TRAINING_DATA: &str = "progression/novice_training.json";
const NOVICE_RECRUIT_DATA: &str = "progression/novice_recruit.json";

const FLOW_ACHIEVEMENT: &str = "achievement";
const FLOW_SEVEN_DAY: &str = "seven_day";
const FLOW_OPEN_SERVER_SIGN: &str = "open_server_sign";
const FLOW_NOVICE_TRAINING: &str = "novice_training";
const FLOW_NOVICE_RECRUIT: &str = "novice_recruit";

/// Combine an achievement id and stage into one claim key.
fn achievement_claim_id(achievement_id: i32, stage: i8) -> i64 {
    i64::from(achievement_id) * 100 + i64::from(stage)
}

/// Handle CS_GAIN_ACHIEVEMENT_AWARD (24022).
pub async fn handle_gain_achievement_award(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_GAIN_ACHIEVEMENT_AWARD,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let table: AchievementTable = GameDataLoader::load_struct(ACHIEVEMENT_DATA)?;
    let award = table
        .awards
        .iter()
        .find(|entry| {
            entry.achievement_id == request.achievement_id && entry.stage == request.stage
        })
        .cloned();

    let mut packets = Vec::new();
    {
        let mut connection = ctx.lock().await;
        let claim_id = achievement_claim_id(request.achievement_id, request.stage);
        match award {
            Some(award) if connection.claim_once(FLOW_ACHIEVEMENT, claim_id) => {
                packets.extend(grant_rewards(&mut connection, &award.reward)?);
                packets.push(build_server_packet(
                    24023,
                    &SC_GAIN_ACHIEVEMENT_AWARD {
                        achievement_id: request.achievement_id,
                        stage: request.stage,
                        point: award.point,
                        result: 1,
                    }
                    .encode(),
                )?);
                packets.push(build_server_packet(
                    24027,
                    &SC_UPDATE_COMPLETE_ACHIEVE_INFO {
                        complete_achieve_info: award.complete.clone(),
                    }
                    .encode(),
                )?);
                packets.push(build_server_packet(
                    24024,
                    &SC_UPDATE_ACHIEVEMENT_INFO {
                        achievement_info: award.next.clone(),
                    }
                    .encode(),
                )?);
                info!(
                    achievement_id = request.achievement_id,
                    stage = request.stage,
                    "Achievement reward claimed"
                );
            }
            _ => {
                // No such reward, or it was claimed already.  The protocol has
                // no sample of this failure path in the capture; report failure
                // with the point total seen so far this session.
                let point = connection.claimed_ids(FLOW_ACHIEVEMENT).len() as i32;
                packets.push(build_server_packet(
                    24023,
                    &SC_GAIN_ACHIEVEMENT_AWARD {
                        achievement_id: request.achievement_id,
                        stage: request.stage,
                        point,
                        result: 0,
                    }
                    .encode(),
                )?);
                info!(
                    achievement_id = request.achievement_id,
                    stage = request.stage,
                    "Achievement reward claim rejected"
                );
            }
        }
    }
    Ok(packets)
}

/// Shared sign-in style claim: seven-day and open-server rewards.
fn claim_day_reward(
    connection: &mut ConnectionContext,
    table: &SignTable,
    flow: &str,
    day: i16,
) -> Result<(Vec<Vec<u8>>, Option<DayReward>), anyhow::Error> {
    let mut packets = Vec::new();
    let entry = table
        .days
        .iter()
        .find(|entry| entry.day == day)
        .cloned();

    let max_day = table.login_day.max(table.open_day);
    if let Some(entry) = entry {
        if day <= max_day && connection.claim_once(flow, i64::from(day)) {
            packets.extend(grant_rewards(connection, &entry.reward)?);
            return Ok((packets, Some(entry)));
        }
    }
    Ok((packets, None))
}

/// Handle CS_GAIN_SEVEN_DAY_REWARD (24065).
pub async fn handle_gain_seven_day_reward(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_GAIN_SEVEN_DAY_REWARD,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let table: SignTable = GameDataLoader::load_struct(SEVEN_DAY_DATA)?;

    let mut packets = Vec::new();
    {
        let mut connection = ctx.lock().await;
        let (reward_packets, claimed) =
            claim_day_reward(&mut connection, &table, FLOW_SEVEN_DAY, request.day)?;
        packets.extend(reward_packets);

        let reward_list = connection.claimed_ids_i16(FLOW_SEVEN_DAY);
        packets.push(build_server_packet(
            24066,
            &SC_SEVEN_DAY_PANEL_INFO {
                login_day: table.login_day,
                seven_day_reward_list: reward_list,
            }
            .encode(),
        )?);
        info!(day = request.day, claimed = claimed.is_some(), "Seven-day reward processed");
    }
    Ok(packets)
}

/// Handle CS_GAIN_OPEN_SERVER_SIGN_REWARD (24270).
pub async fn handle_gain_open_server_sign_reward(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_GAIN_OPEN_SERVER_SIGN_REWARD,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let table: SignTable = GameDataLoader::load_struct(OPEN_SERVER_SIGN_DATA)?;

    let mut packets = Vec::new();
    {
        let mut connection = ctx.lock().await;
        let (reward_packets, claimed) = claim_day_reward(
            &mut connection,
            &table,
            FLOW_OPEN_SERVER_SIGN,
            request.day,
        )?;
        packets.extend(reward_packets);

        let reward_list = connection.claimed_ids_i16(FLOW_OPEN_SERVER_SIGN);
        packets.push(build_server_packet(
            24271,
            &SC_OPEN_SERVER_SIGN_PANEL_INFO {
                open_day: table.open_day,
                end_time: table.end_time,
                sign_day_reward_list: reward_list,
            }
            .encode(),
        )?);
        info!(
            day = request.day,
            claimed = claimed.is_some(),
            "Open-server sign reward processed"
        );
    }
    Ok(packets)
}

/// Patch the novice-training panel with the current claim state.
fn novice_training_panel_packet(
    table: &NoviceTrainingTable,
    connection: &ConnectionContext,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let Some(panel) = &table.panel else {
        anyhow::bail!(
            "novice training table {} has no panel entry",
            NOVICE_TRAINING_DATA
        );
    };
    let mut panel = panel.clone();
    for task in panel.task_list.iter_mut() {
        if connection.is_claimed(FLOW_NOVICE_TRAINING, i64::from(task.id)) {
            task.state = 2;
        }
    }
    Ok(vec![build_server_packet(24112, &panel.encode())?])
}

/// Handle CS_NOVICE_TRAINING_PANEL (24111): report the current task panel.
pub async fn handle_novice_training_panel(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_NOVICE_TRAINING_PANEL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let table: NoviceTrainingTable = GameDataLoader::load_struct(NOVICE_TRAINING_DATA)?;
    let connection = ctx.lock().await;
    novice_training_panel_packet(&table, &connection)
}

/// Handle CS_NOVICE_TRAINING_RECEIVE_TASK (24113): claim training tasks.
///
/// The capture does not contain a direct reply for this command (the claim
/// results arrived with the next panel request), so the emulator answers
/// immediately with the same messages the server later pushed.
pub async fn handle_novice_training_receive_task(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_NOVICE_TRAINING_RECEIVE_TASK,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let table: NoviceTrainingTable = GameDataLoader::load_struct(NOVICE_TRAINING_DATA)?;

    let mut packets = Vec::new();
    let mut accepted: Vec<i16> = Vec::new();
    {
        let mut connection = ctx.lock().await;
        for task_id in request.task_id_list.iter().copied() {
            let Some(task) = table
                .tasks
                .iter()
                .find(|entry| entry.id == task_id)
                .cloned()
            else {
                continue;
            };
            if !connection.claim_once(FLOW_NOVICE_TRAINING, i64::from(task_id)) {
                continue;
            }
            packets.extend(grant_rewards(&mut connection, &task.reward)?);
            accepted.push(task_id);
        }

        packets.push(build_server_packet(
            24114,
            &SC_NOVICE_TRAINING_RECEIVE_TASK {
                task_id_list: accepted.clone(),
                result: if accepted.is_empty() { 0 } else { 1 },
            }
            .encode(),
        )?);
        packets.extend(novice_training_panel_packet(&table, &connection)?);
        info!(tasks = ?accepted, "Novice training tasks claimed");
    }
    Ok(packets)
}

/// Handle CS_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE (24221).
pub async fn handle_novice_recruit_receive(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let table: RecruitTable = GameDataLoader::load_struct(NOVICE_RECRUIT_DATA)?;

    let mut packets = Vec::new();
    {
        let mut connection = ctx.lock().await;
        let reward = table
            .rewards
            .iter()
            .find(|entry| entry.id == request.id)
            .cloned();

        match reward {
            Some(reward) if connection.claim_once(FLOW_NOVICE_RECRUIT, i64::from(request.id)) => {
                packets.extend(unread_packets(&reward.reward.unread)?);
                packets.extend(grant_rewards(&mut connection, &reward.reward)?);
                packets.push(build_server_packet(
                    24222,
                    &SC_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE {
                        result: 1,
                        id: request.id,
                    }
                    .encode(),
                )?);
                info!(id = request.id, "Novice recruit reward claimed");
            }
            _ => {
                packets.push(build_server_packet(
                    24222,
                    &SC_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE {
                        result: 0,
                        id: request.id,
                    }
                    .encode(),
                )?);
                info!(id = request.id, "Novice recruit reward claim rejected");
            }
        }
    }
    Ok(packets)
}

/// Recruit rewards table: one entry per claimable id.
#[derive(Clone, Debug, Default, Deserialize)]
struct RecruitTable {
    #[serde(default)]
    rewards: Vec<IdReward>,
    #[serde(default)]
    recruit_times: i16,
}


const FUND_GAIN_DATA: &str = "progression/fund_gain.json";
const DAILY_SIGN_DATA: &str = "progression/daily_sign.json";

/// Replay a recorded single-group template and absorb attribute updates.
async fn replay_first_group(
    ctx: &Arc<Mutex<ConnectionContext>>,
    path: &str,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(path)?;
    let Some(group) = script.first_group().cloned() else {
        return Ok(Vec::new());
    };
    let cursor = ctx.lock().await.replay_cursor.clone();
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        crate::cmd::battle::absorb_attr_updates(&mut connection, &group);
    }
    Ok(packets)
}

/// Handle CS_GAIN_ALL_FUND (24207): replay the recorded fund claim.
pub async fn handle_gain_all_fund(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_GAIN_ALL_FUND,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!("Fund reward claimed (replay)");
    replay_first_group(&ctx, FUND_GAIN_DATA).await
}

/// Handle CS_DAILY_SIGN (24034): replay the recorded daily sign-in.
pub async fn handle_daily_sign(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_DAILY_SIGN,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!("Daily sign-in (replay)");
    replay_first_group(&ctx, DAILY_SIGN_DATA).await
}
