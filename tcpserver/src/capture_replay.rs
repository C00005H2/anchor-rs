//! Best-effort replay for decoded response groups from proxy JSONL captures.
//!
//! This intentionally re-encodes decoded messages instead of storing raw wire
//! payloads: captures contain parsed JSON, not the original server frames.

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use crate::{messages::*, packet::build_server_packet};

const MARKER_ACCOUNT_ID: &str = "__ANCHOR_REPLAY_ACCOUNT_ID__";
const MARKER_PLAYER_ID: &str = "__ANCHOR_REPLAY_PLAYER_ID__";
const MARKER_SESSION: &str = "__ANCHOR_REPLAY_SESSION__";

#[derive(Debug, Deserialize)]
struct CaptureRow {
    client_request: CaptureCommand,
    #[serde(default)]
    server_responses: Vec<CaptureResponseRow>,
}

#[derive(Debug, Deserialize)]
struct CaptureCommand {
    cmd: u32,
}

#[derive(Debug, Deserialize)]
struct CaptureResponseRow {
    cmd: u32,
    #[serde(default)]
    decoded: Option<Value>,
    /// Raw payload of a server message that has no schema in `messages.rs`.
    #[serde(default)]
    payload_hex: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CapturedResponse {
    pub cmd: u32,
    /// JSON representation produced by `dispatch_cmd`, or `Value::Null` when the
    /// message has no schema and only `raw` bytes are available.
    pub decoded: Value,
    /// Original wire payload, only present for messages without a schema.
    pub raw: Option<Vec<u8>>,
}

/// Decode the `payload_hex` capture field. A trailing `...` (truncated payload)
/// makes the payload unusable, and is rejected.
pub(crate) fn decode_payload_hex(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || text.ends_with("...") || text.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let digits = text.as_bytes();
    for pair in digits.chunks(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        bytes.push(((high << 4) | low) as u8);
    }
    Some(bytes)
}

#[derive(Clone, Debug, Default)]
pub struct CapturedRequestGroup {
    pub responses: Vec<CapturedResponse>,
}

#[derive(Debug, Default)]
pub struct CaptureReplay {
    groups_by_command: HashMap<u32, Vec<CapturedRequestGroup>>,
    skipped_undecoded_responses: usize,
    /// `SC_SYS_DATE.time` of the recorded session, used to shift timestamps.
    captured_time: Option<i64>,
}

#[derive(Debug)]
pub enum ReplayLookup {
    /// This command was not present in the capture; normal handlers may run.
    NotCaptured,
    /// The captured entries for this command have already been consumed.
    Exhausted,
    /// A captured request group, which may intentionally have no responses.
    Group(CapturedRequestGroup),
}

#[derive(Clone, Debug)]
pub struct ReplayCursor {
    next_group_by_command: HashMap<u32, usize>,
    account_id: String,
    player_id: String,
    session: String,
    /// Seconds added to captured server timestamps so a replay reports "now".
    /// `None` until the capture's own reference time is known.
    time_offset: Option<i64>,
}

impl Default for ReplayCursor {
    fn default() -> Self {
        let account_id = random_decimal_id();
        let mut player_id = random_decimal_id();
        while player_id == account_id {
            player_id = random_decimal_id();
        }

        Self {
            next_group_by_command: HashMap::new(),
            account_id,
            player_id,
            session: format!("{:032x}", rand::random::<u128>()),
            time_offset: None,
        }
    }
}

impl ReplayCursor {
    /// Replace capture-specific account/session values with per-connection IDs
    /// and move captured server timestamps to the current time.
    pub fn rehydrate(&self, value: &Value) -> Value {
        let mut value = value.clone();
        rehydrate_value(
            &mut value,
            &self.account_id,
            &self.player_id,
            &self.session,
            self.time_offset.unwrap_or(0),
        );
        value
    }

    /// Start reporting timestamps relative to `captured_time`.
    fn anchor_to_time(&mut self, captured_time: i64, now: i64) {
        if self.time_offset.is_none() {
            self.time_offset = Some(now - captured_time);
        }
    }
}

fn random_decimal_id() -> String {
    let id = rand::random::<u64>() & i64::MAX as u64;
    id.max(1).to_string()
}

impl CaptureReplay {
    /// Load one proxy JSONL capture. Client payloads are deliberately ignored;
    /// in particular, credentials in the client login request are never kept.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("could not open capture file {}", path.display()))?;
        let replay = Self::from_reader(BufReader::new(file))
            .with_context(|| format!("could not load capture file {}", path.display()))?;

        let group_count: usize = replay.groups_by_command.values().map(Vec::len).sum();
        tracing::info!(
            commands = replay.groups_by_command.len(),
            request_groups = group_count,
            skipped_undecoded_responses = replay.skipped_undecoded_responses,
            "Loaded decoded capture for replay"
        );
        Ok(replay)
    }

    fn from_reader(reader: impl BufRead) -> anyhow::Result<Self> {
        let mut replay = Self::default();
        let mut line_count = 0usize;

        for (line_index, line) in reader.lines().enumerate() {
            let line = line.with_context(|| format!("could not read JSONL line {}", line_index + 1))?;
            if line.trim().is_empty() {
                continue;
            }

            let row: CaptureRow = serde_json::from_str(&line)
                .with_context(|| format!("invalid capture JSON on line {}", line_index + 1))?;
            let mut responses = Vec::with_capacity(row.server_responses.len());

            for response in row.server_responses {
                let raw = response
                    .payload_hex
                    .as_deref()
                    .and_then(decode_payload_hex);
                if response.cmd == 10002 {
                    if let Some(seconds) = response
                        .decoded
                        .as_ref()
                        .and_then(|decoded| decoded.get("time"))
                        .and_then(|time| time.as_i64())
                    {
                        replay.captured_time.get_or_insert(seconds);
                    }
                }
                match response.decoded {
                    Some(mut decoded) if !decoded.is_null() => {
                        redact_capture_value(response.cmd, &mut decoded);
                        responses.push(CapturedResponse {
                            cmd: response.cmd,
                            decoded,
                            raw: None,
                        });
                    }
                    _ => match raw {
                        // A message without a schema can still be replayed when
                        // the capture stored its original bytes.
                        Some(bytes) => responses.push(CapturedResponse {
                            cmd: response.cmd,
                            decoded: Value::Null,
                            raw: Some(bytes),
                        }),
                        None => replay.skipped_undecoded_responses += 1,
                    },
                }
            }

            replay
                .groups_by_command
                .entry(row.client_request.cmd)
                .or_default()
                .push(CapturedRequestGroup { responses });
            line_count += 1;
        }

        if line_count == 0 {
            anyhow::bail!("capture file contains no request groups");
        }
        Ok(replay)
    }

    pub fn next_group(&self, cursor: &mut ReplayCursor, command: u32) -> ReplayLookup {
        let Some(groups) = self.groups_by_command.get(&command) else {
            return ReplayLookup::NotCaptured;
        };

        if let Some(captured_time) = self.captured_time {
            cursor.anchor_to_time(captured_time, chrono::Utc::now().timestamp());
        }

        let next = cursor.next_group_by_command.entry(command).or_default();
        let Some(group) = groups.get(*next) else {
            return ReplayLookup::Exhausted;
        };
        *next += 1;
        ReplayLookup::Group(group.clone())
    }
}

/// Redact credentials and user-identifying fields before a capture is stored
/// or replayed. The ID/session markers are replaced with fresh per-connection
/// values by `ReplayCursor`.
pub(crate) fn redact_capture_value(command: u32, value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                match key.as_str() {
                    "acc_id" => *field = Value::String(MARKER_ACCOUNT_ID.to_owned()),
                    "player_id" => *field = Value::String(MARKER_PLAYER_ID.to_owned()),
                    "session" => *field = Value::String(MARKER_SESSION.to_owned()),
                    "login_token" | "dev_token" | "dev_code" | "dev_model" | "acc_name" => {
                        *field = Value::String("[redacted]".to_owned())
                    }
                    "player_name" => *field = Value::String("Replay Player".to_owned()),
                    "sender_name" => *field = Value::String("Replay User".to_owned()),
                    "sender_id" | "show_id" => *field = Value::String("0".to_owned()),
                    "signature" | "player_signature" | "friend_remarks" => {
                        *field = Value::String(String::new())
                    }
                    "content" if matches!(command, 10050 | 10051) => {
                        *field = Value::String("[captured chat omitted]".to_owned())
                    }
                    _ => redact_capture_value(command, field),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_capture_value(command, item);
            }
        }
        _ => {}
    }
}

/// Server timestamps that describe "now" rather than a fixed game date.
const SHIFTED_TIME_FIELDS: [&str; 2] = ["time", "next_refresh_time"];

fn rehydrate_value(
    value: &mut Value,
    account_id: &str,
    player_id: &str,
    session: &str,
    time_offset: i64,
) {
    match value {
        Value::String(text) if text == MARKER_ACCOUNT_ID => *text = account_id.to_owned(),
        Value::String(text) if text == MARKER_PLAYER_ID => *text = player_id.to_owned(),
        Value::String(text) if text == MARKER_SESSION => *text = session.to_owned(),
        Value::Array(items) => {
            for item in items {
                rehydrate_value(item, account_id, player_id, session, time_offset);
            }
        }
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                if time_offset != 0 && SHIFTED_TIME_FIELDS.contains(&key.as_str()) {
                    if let Some(seconds) = field.as_i64() {
                        *field = Value::from(seconds.saturating_add(time_offset));
                        continue;
                    }
                }
                rehydrate_value(field, account_id, player_id, session, time_offset);
            }
        }
        _ => {}
    }
}

/// Convert the JSON representation emitted by `dispatch_cmd` back to a server
/// packet using the generated message encoder. Unknown message IDs have no
/// schema in this repository and are skipped by the caller.
pub fn encode_captured_response(response: &CapturedResponse) -> anyhow::Result<Option<Vec<u8>>> {
    let cmd_id = response.cmd;
    if response.decoded.is_null() {
        return match &response.raw {
            Some(payload) => Ok(Some(build_server_packet(cmd_id, payload)?)),
            None => Ok(None),
        };
    }
    let decoded = response.decoded.clone();

    macro_rules! encode_as {
        ($message:ty) => {{
            let message: $message = serde_json::from_value(decoded).with_context(|| {
                format!(
                    "captured response {cmd_id} does not match {}",
                    stringify!($message)
                )
            })?;
            let payload = message.encode();
            Ok(Some(build_server_packet(cmd_id, &payload)?))
        }};
    }

    match cmd_id {
        10002 => encode_as!(SC_SYS_DATE),
        10021 => encode_as!(SC_PAY_DATA),
        10032 => encode_as!(SC_SYSTEM_ANNOUNCE),
        10051 => encode_as!(SC_PUBLIC_CHAT),
        10055 => encode_as!(SC_PUBLIC_CHAT_SETTING),
        10056 => encode_as!(SC_RES_ALL_MODULE_READ),
        10058 => encode_as!(SC_RES_MODULE_READ),
        10059 => encode_as!(SC_NEW_UNREAD),
        10071 => encode_as!(SC_GET_SERVER_STATE),
        10101 => encode_as!(SC_SETTING),
        11001 => encode_as!(SC_ACCOUNT_LOGIN),
        12000 => encode_as!(SC_PLAYER_END_DATA),
        12001 => encode_as!(SC_PLAYER_BASE_DATA),
        12002 => encode_as!(SC_PLAYER_UPDATE_ATTR_INT),
        12003 => encode_as!(SC_PLAYER_UPDATE_ATTR_BIGINT),
        12006 => encode_as!(SC_TODAY_NOT_NOTICE),
        12009 => encode_as!(SC_FUNCTION_OPEN_LIST),
        12010 => encode_as!(SC_ADD_FUNCTION_OPEN),
        12018 => encode_as!(SC_GET_AVATAR_LIST),
        12020 => encode_as!(SC_GET_AVATAR_FRAME_LIST),
        12022 => encode_as!(SC_GET_DESIGNATION_LIST),
        12034 => encode_as!(SC_PLAYER_HOMEPAGE_INFO),
        12036 => encode_as!(SC_GET_DIALOG_BOX_LIST),
        12051 => encode_as!(SC_PLAYER_STAMINA_INFO),
        12053 => encode_as!(SC_PLAYER_STORY_INFO),
        12055 => encode_as!(SC_TITANIUM_EXCHANGE_GOLD_COIN_INFO),
        12058 => encode_as!(SC_PLAYER_GUIDE_INFO),
        12062 => encode_as!(SC_IS_RENAME),
        12067 => encode_as!(SC_GET_BACKGROUND_LIST),
        12100 => encode_as!(SC_MONSTER_MANUAL),
        12101 => encode_as!(SC_EQUIP_SUIT_MANUAL),
        12102 => encode_as!(SC_BRACELET_MANUAL),
        12103 => encode_as!(SC_MUSIC_MANUAL),
        12104 => encode_as!(SC_STORY_MANUAL),
        12105 => encode_as!(SC_WORLD_MANUAL),
        12160 => encode_as!(SC_DIALOGUE_PANEL),
        13001 => encode_as!(SC_UPDATE_HERO_LIST),
        13003 => encode_as!(SC_HERO_UPDATE_ATTR),
        13007 => encode_as!(SC_HERO_LEVELUP),
        13011 => encode_as!(SC_HERO_DETAIL),
        13041 => encode_as!(SC_HERO_FORMATION),
        13045 => encode_as!(SC_SET_READY),
        13047 => encode_as!(SC_CHANGE_HERO),
        13050 => encode_as!(SC_RECRUIT_INFO),
        13052 => encode_as!(SC_RECRUIT_ITEM),
        13060 => encode_as!(SC_HERO_PRE_LIST),
        13062 => encode_as!(SC_CANNOT_DEL_HERO_LIST),
        13090 => encode_as!(SC_HERO_UPDATE_ATTR_BIGINT),
        13107 => encode_as!(SC_FASHION_INFO),
        13142 => encode_as!(SC_RELATION_REWARD),
        13200 => encode_as!(SC_HERO_LV_REWARD),
        13220 => encode_as!(SC_HERO_ASSIST_FIGHT_SKILL),
        13280 => encode_as!(SC_HERO_ACTION_LIST),
        13291 => encode_as!(SC_RECRUIT_HERO_NEW_SAVE_LIST),
        13350 => encode_as!(SC_HERO_FASHION_HAVE_INFO),
        13362 => encode_as!(SC_ACT_FETTER_INFO),
        13370 => encode_as!(SC_FASHION_SCENE_PANEL),
        15001 => encode_as!(SC_FRIEND_LIST),
        15003 => encode_as!(SC_FRIEND_APPLY_LIST),
        15013 => encode_as!(SC_BLACK_LIST),
        15028 => encode_as!(SC_FRIEND_GIFT_PANEL),
        16001 => encode_as!(SC_MAIL_LIST),
        16002 => encode_as!(SC_MAIL_ADD),
        16006 => encode_as!(SC_MAIL_READ),
        16008 => encode_as!(SC_MAIL_ENCLOSURE_REC),
        17000 => encode_as!(SC_BAG_INIT),
        17001 => encode_as!(SC_BAG_UPDATE),
        17006 => encode_as!(SC_SHOP_DATA),
        17010 => encode_as!(SC_SHOP_TYPE_DATA),
        17013 => encode_as!(SC_PROP_AWARD_SEND),
        18000 => encode_as!(SC_MAIN_STORY_INFO),
        18001 => encode_as!(SC_DUP_DATA),
        18054 => encode_as!(SC_MAIN_STORY_STAGE_AWARD_LIST),
        18062 => encode_as!(SC_CHIP_DUP_PICK_SUIT),
        18120 => encode_as!(SC_PARKOUR_PANEL),
        19002 => encode_as!(SC_COLLEGE_DELEGATION_HALL_INFO),
        19050 => encode_as!(SC_ACTIVITY_OPEN_INFO),
        19456 => encode_as!(SC_WAR_SHIP_UPDATE_HERO),
        19601 => encode_as!(SC_HERO_TRY_INFO),
        20101 => encode_as!(SC_BATTLE_FIELD_INFO),
        20103 => encode_as!(SC_BATTLE_ACTION),
        20105 => encode_as!(SC_BATTLE_ACTION_END),
        20106 => encode_as!(SC_BATTLE_RESULT),
        20111 => encode_as!(SC_BATTLE_REPLAY_INFOS),
        20114 => encode_as!(SC_BATTLE_AUTO),
        20125 => encode_as!(SC_BATTLE_ACTION_NOTICE),
        21001 => encode_as!(SC_FORCES_PANEL),
        21073 => encode_as!(SC_UPDATE_FORCES_TASK_INFO),
        23002 => encode_as!(SC_GUILD_PANEL),
        23008 => encode_as!(SC_REFRESH_RECOMMEND_GUILDS),
        24007 => encode_as!(SC_UPDATE_TASK_INFO),
        24021 => encode_as!(SC_ACHIEVEMENT_PANEL_INFO),
        24023 => encode_as!(SC_GAIN_ACHIEVEMENT_AWARD),
        24024 => encode_as!(SC_UPDATE_ACHIEVEMENT_INFO),
        24027 => encode_as!(SC_UPDATE_COMPLETE_ACHIEVE_INFO),
        24033 => encode_as!(SC_SIGN_PANEL),
        24035 => encode_as!(SC_DAILY_SIGN),
        24041 => encode_as!(SC_NOVICE_TARGET_PANEL_INFO),
        24066 => encode_as!(SC_SEVEN_DAY_PANEL_INFO),
        24095 => encode_as!(SC_MONTH_CARD_PANEL),
        24097 => encode_as!(SC_DIRECT_GIFT_PANEL),
        24099 => encode_as!(SC_DIRECT_GIFT_BUY),
        24106 => encode_as!(SC_ACC_PAY_PANEL),
        24112 => encode_as!(SC_NOVICE_TRAINING_PANEL),
        24114 => encode_as!(SC_NOVICE_TRAINING_RECEIVE_TASK),
        24117 => encode_as!(SC_UPDATE_NOVICE_TRAINING_TASK),
        24131 => encode_as!(SC_FASHION_SHOP_PANEL),
        24201 => encode_as!(SC_LEVEL_GIFT_PANEL),
        24206 => encode_as!(SC_FUND_PANEL),
        24212 => encode_as!(SC_FIRST_PAY_PANEL),
        24220 => encode_as!(SC_ACTIVITY_NOVICE_RECRUIT_HERO_PANEL),
        24222 => encode_as!(SC_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE),
        24240 => encode_as!(SC_ACTIVITY_NOVICE_UPGRADE_PANEL),
        24250 => encode_as!(SC_ACTIVITY_NOVICE_START),
        24271 => encode_as!(SC_OPEN_SERVER_SIGN_PANEL_INFO),
        24276 => encode_as!(SC_PLATFORM_ACHIEVE_PANEL),
        24351 => encode_as!(SC_STAMINA_MONTH_CARD_PANEL),
        24372 => encode_as!(SC_DOWN_GIFT_SHOW),
        24402 => encode_as!(SC_LIMITED_GIFT_PANEL),
        24462 => encode_as!(SC_ACTIVITY_DAY_REWARD_PANEL),
        24490 => encode_as!(SC_ACTIVITY_EXPIRED_GOODS),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatch::dispatch_cmd, packet::parse_server_packet};
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn replay_groups_are_consumed_in_order_per_command() {
        let capture = concat!(
            r#"{"client_request":{"cmd":10054,"decoded":{"channel":1}},"server_responses":[{"cmd":10055,"decoded":{"channel":1,"room_now":2,"people_count":3,"room_list":[1,2]}}]}"#,
            "\n",
            r#"{"client_request":{"cmd":10054,"decoded":{"channel":2}},"server_responses":[]}"#,
        );
        let replay = CaptureReplay::from_reader(Cursor::new(capture.as_bytes())).unwrap();
        let mut cursor = ReplayCursor::default();

        match replay.next_group(&mut cursor, 10054) {
            ReplayLookup::Group(group) => assert_eq!(group.responses[0].cmd, 10055),
            _ => panic!("expected the first captured group"),
        }
        match replay.next_group(&mut cursor, 10054) {
            ReplayLookup::Group(group) => assert!(group.responses.is_empty()),
            _ => panic!("empty response groups must still be replayed"),
        }
        assert!(matches!(
            replay.next_group(&mut cursor, 10054),
            ReplayLookup::Exhausted
        ));
        assert!(matches!(
            replay.next_group(&mut cursor, 11005),
            ReplayLookup::NotCaptured
        ));
    }

    #[test]
    fn captured_json_reencodes_as_a_valid_server_packet() {
        let response = CapturedResponse {
            cmd: 10002,
            decoded: json!({"time": 123, "open_date": 100, "merge_date": 0}),
            raw: None,
        };
        let packet = encode_captured_response(&response).unwrap().unwrap();
        let (cmd, body) = parse_server_packet(&packet, "").unwrap();
        assert_eq!(cmd, 10002);
        assert_eq!(
            dispatch_cmd(cmd, &body),
            Some(json!({"time": 123, "open_date": 100, "merge_date": 0}))
        );
    }

    #[test]
    fn undecoded_payloads_replay_from_raw_bytes() {
        let capture = concat!(
            r#"{"client_request":{"cmd":18006,"decoded":null},"server_responses":[{"cmd":19910,"decoded":null,"payload_hex":"0001"}]}"#,
            "\n",
            r#"{"client_request":{"cmd":18006,"decoded":null},"server_responses":[{"cmd":19911,"decoded":null,"payload_hex":"00"}]}"#,
        );
        let replay = CaptureReplay::from_reader(Cursor::new(capture.as_bytes())).unwrap();
        let mut cursor = ReplayCursor::default();

        match replay.next_group(&mut cursor, 18006) {
            ReplayLookup::Group(group) => {
                assert_eq!(group.responses[0].raw.as_deref(), Some(&[0u8, 1u8][..]));
                let packet = encode_captured_response(&group.responses[0])
                    .unwrap()
                    .unwrap();
                let (cmd, body) = parse_server_packet(&packet, "").unwrap();
                assert_eq!(cmd, 19910);
                assert_eq!(body, vec![0u8, 1]);
            }
            _ => panic!("expected a captured group with raw bytes"),
        }

        // Truncated payloads cannot be replayed and are skipped instead.
        match replay.next_group(&mut cursor, 18006) {
            ReplayLookup::Group(group) => assert!(group.responses.is_empty()),
            _ => panic!("expected the second captured group"),
        }
    }

    #[test]
    fn payload_hex_decoding_rejects_truncated_and_odd_input() {
        assert_eq!(decode_payload_hex("0102ff"), Some(vec![1, 2, 255]));
        assert_eq!(decode_payload_hex("0102..."), None);
        assert_eq!(decode_payload_hex("0"), None);
        assert_eq!(decode_payload_hex("zz"), None);
        assert_eq!(decode_payload_hex(""), None);
    }

    #[test]
    fn sensitive_capture_fields_are_anonymized_and_rehydrated() {
        let mut value = json!({
            "result": 0,
            "acc_id": "1234",
            "player_id": "5678",
            "is_new": 0,
            "session": "old-session",
            "create_time": 123,
            "login_token": "do-not-store",
            "player_name": "Private Name",
            "signature": "private text"
        });
        redact_capture_value(11001, &mut value);
        assert_eq!(value["login_token"], "[redacted]");
        assert_eq!(value["player_name"], "Replay Player");
        assert_eq!(value["signature"], "");

        let cursor = ReplayCursor::default();
        let replayed = cursor.rehydrate(&value);
        assert_ne!(replayed["acc_id"], MARKER_ACCOUNT_ID);
        assert_ne!(replayed["player_id"], MARKER_PLAYER_ID);
        assert_ne!(replayed["session"], MARKER_SESSION);
        assert_eq!(replayed["login_token"], "[redacted]");

        let login = CapturedResponse {
            cmd: 11001,
            decoded: replayed,
            raw: None,
        };
        let packet = encode_captured_response(&login).unwrap().unwrap();
        let (cmd, body) = parse_server_packet(&packet, "").unwrap();
        let decoded = dispatch_cmd(cmd, &body).unwrap();
        assert!(decoded["acc_id"].as_str().unwrap().parse::<i64>().is_ok());
        assert!(decoded["player_id"].as_str().unwrap().parse::<i64>().is_ok());
        assert_ne!(decoded["session"], "old-session");
    }
}
