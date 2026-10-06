//! Reward tables and shared grant logic for progression flows.
//!
//! The captured session defines what each claim gives.  `tools/import_capture_data.py`
//! extracts those rewards into JSON tables under `DATA_DIR/progression/`; the
//! handlers below apply them to the local player profile so claims behave like
//! real transactions (once per id, bag items merged, attribute deltas applied)
//! instead of replaying recorded bytes.

use serde::Deserialize;

use crate::{
    messages::{
        pt_achievement_info, pt_attr_bigint, pt_complete_achieve_info, pt_prop_award,
        pt_prop_bag, SC_BAG_UPDATE, SC_NEW_UNREAD, SC_PLAYER_UPDATE_ATTR_BIGINT,
        SC_PROP_AWARD_SEND,
    },
    packet::build_server_packet,
    state::ConnectionContext,
};

/// One `SC_NEW_UNREAD` notice stored in a reward table.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct UnreadNotice {
    #[serde(rename = "type")]
    pub msg_type: i32,
    #[serde(default)]
    pub id_list: Vec<i32>,
}

/// What a claim grants.  `attr_delta` stores per-key changes; the captured
/// absolute `attr_list` values are only used when no local value is known.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RewardTable {
    #[serde(default)]
    pub award_list: Vec<pt_prop_award>,
    /// Template bag items; ids and creation times are assigned on grant.
    #[serde(default)]
    pub bag_items: Vec<pt_prop_bag>,
    /// Captured absolute attribute values after the claim.
    #[serde(default)]
    pub attr_list: Vec<pt_attr_bigint>,
    /// Attribute deltas (`value = after - before` in the capture).
    #[serde(default)]
    pub attr_delta: Vec<pt_attr_bigint>,
    #[serde(default)]
    pub unread: Vec<UnreadNotice>,
}

/// A day-indexed reward (seven-day sign-in, server-open sign-in).
#[derive(Clone, Debug, Deserialize)]
pub struct DayReward {
    pub day: i16,
    #[serde(flatten)]
    pub reward: RewardTable,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SignTable {
    #[serde(default)]
    pub login_day: i16,
    #[serde(default)]
    pub open_day: i16,
    #[serde(default)]
    pub end_time: i32,
    #[serde(default)]
    pub days: Vec<DayReward>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AchievementAward {
    pub achievement_id: i32,
    pub stage: i8,
    pub point: i32,
    #[serde(flatten)]
    pub reward: RewardTable,
    /// Achievement state after claiming.
    pub next: pt_achievement_info,
    #[serde(default)]
    pub complete: Vec<pt_complete_achieve_info>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AchievementTable {
    #[serde(default)]
    pub awards: Vec<AchievementAward>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GoodsAward {
    pub goods_id: i32,
    #[serde(default)]
    pub num: i16,
    #[serde(flatten)]
    pub reward: RewardTable,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct GiftTable {
    #[serde(default)]
    pub goods: Vec<GoodsAward>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IdReward {
    pub id: i16,
    #[serde(flatten)]
    pub reward: RewardTable,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NoviceTrainingTable {
    /// Claimable tasks with their rewards.
    #[serde(default)]
    pub tasks: Vec<IdReward>,
    /// Recruit times shown on recruit-related panels.
    #[serde(default)]
    pub recruit_times: i16,
    /// Panel sent together with claim results.
    pub panel: Option<crate::messages::SC_NOVICE_TRAINING_PANEL>,
}

/// Grant the captured rewards: bag update, attribute updates, then the award
/// notification — the order the real server sends them in.
pub fn grant_rewards(
    connection: &mut ConnectionContext,
    reward: &RewardTable,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut packets = Vec::new();

    if !reward.bag_items.is_empty() {
        let now = chrono::Utc::now().timestamp() as i32;
        let mut update_list = Vec::with_capacity(reward.bag_items.len());
        for template in &reward.bag_items {
            // Stacks merge by template id, like the real server: granting an
            // item the profile already owns grows that stack instead of
            // creating a second entry.
            let (id, count, color) =
                connection.grant_bag_item(template.tid, template.count, template.color);
            let mut item = template.clone();
            item.id = id;
            item.count = count;
            item.color = color;
            item.createdTime = now;
            update_list.push(item);
        }
        packets.push(build_server_packet(
            17001,
            &SC_BAG_UPDATE {
                msg_type: 1,
                updateList: update_list,
                delList: Vec::new(),
            }
            .encode(),
        )?);
    }

    packets.extend(apply_attr_reward(connection, reward)?);

    if !reward.award_list.is_empty() {
        packets.push(build_server_packet(
            17013,
            &SC_PROP_AWARD_SEND {
                award_list: reward.award_list.clone(),
            }
            .encode(),
        )?);
    }

    Ok(packets)
}

/// Apply attribute rewards.  Deltas are applied on top of the local profile so
/// repeated sessions keep adding; the captured absolute values are the fallback
/// for attributes the profile has never seen.
fn apply_attr_reward(
    connection: &mut ConnectionContext,
    reward: &RewardTable,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut packets = Vec::new();
    for after in &reward.attr_list {
        let delta = reward
            .attr_delta
            .iter()
            .find(|entry| entry.key == after.key)
            .and_then(|entry| entry.value.parse::<i64>().ok());

        let new_value = match (delta, connection.attr_value(after.key)) {
            (Some(delta), Some(current)) => current
                .parse::<i64>()
                .map(|value| value.saturating_add(delta).to_string())
                .unwrap_or_else(|_| after.value.clone()),
            _ => after.value.clone(),
        };

        connection.record_attr(after.key, new_value.clone());
        packets.push(build_server_packet(
            12003,
            &SC_PLAYER_UPDATE_ATTR_BIGINT {
                attr_list: vec![pt_attr_bigint {
                    key: after.key,
                    value: new_value,
                }],
            }
            .encode(),
        )?);
    }
    Ok(packets)
}

/// Build the `SC_NEW_UNREAD` notices recorded for a flow.
pub fn unread_packets(notices: &[UnreadNotice]) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut packets = Vec::with_capacity(notices.len());
    for notice in notices {
        packets.push(build_server_packet(
            10059,
            &SC_NEW_UNREAD {
                msg_type: notice.msg_type,
                id_list: notice.id_list.clone(),
            }
            .encode(),
        )?);
    }
    Ok(packets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reward_with_attrs(after: &[(&str, &str)], delta: &[(&str, &str)]) -> RewardTable {
        RewardTable {
            attr_list: after
                .iter()
                .map(|(key, value)| pt_attr_bigint {
                    key: key.parse().unwrap(),
                    value: (*value).to_owned(),
                })
                .collect(),
            attr_delta: delta
                .iter()
                .map(|(key, value)| pt_attr_bigint {
                    key: key.parse().unwrap(),
                    value: (*value).to_owned(),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn attr_deltas_accumulate_on_the_local_profile() {
        let mut connection = ConnectionContext::new("session".to_owned());
        connection.record_attr(611, "110".to_owned());

        let packets = apply_attr_reward(
            &mut connection,
            &reward_with_attrs(&[("611", "150")], &[("611", "40")]),
        )
        .unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(connection.attr_value(611), Some("150"));

        // A second identical claim adds the delta again.
        apply_attr_reward(
            &mut connection,
            &reward_with_attrs(&[("611", "190")], &[("611", "40")]),
        )
        .unwrap();
        assert_eq!(connection.attr_value(611), Some("190"));
    }

    #[test]
    fn unknown_attributes_fall_back_to_captured_values() {
        let mut connection = ConnectionContext::new("session".to_owned());
        apply_attr_reward(
            &mut connection,
            &reward_with_attrs(&[("612", "41800")], &[("612", "30000")]),
        )
        .unwrap();
        assert_eq!(connection.attr_value(612), Some("41800"));
    }

    #[test]
    fn non_numeric_attribute_values_pass_through() {
        let mut connection = ConnectionContext::new("session".to_owned());
        connection.record_attr(611, "not-a-number".to_owned());
        apply_attr_reward(
            &mut connection,
            &reward_with_attrs(&[("611", "150")], &[("611", "40")]),
        )
        .unwrap();
        assert_eq!(connection.attr_value(611), Some("150"));
    }
}
