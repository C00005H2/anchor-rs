use serde_json;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use common::DATA_DIRECTORY;
use crate::messages::*;
use crate::packet::build_server_packet;

macro_rules! load_raw {
    ($path:expr, $id:expr, $packets:ident) => {
        $packets.push(
            Self::load_raw_packet($path, $id)
                .with_context(|| format!("Failed to load raw packet {}", $path))?
        );
    };
}

macro_rules! load_packet {
    ($ty:ty, $path:expr, $id:expr, $packets:ident) => {
        $packets.push(
            Self::build_packet::<$ty>($path, $id)
                .with_context(|| format!("Failed to load {} for {}", stringify!($ty), $path))?
        );
    };
}

pub struct GameDataLoader;

impl GameDataLoader {
    /// Load any message struct from a JSON file under `DATA_DIRECTORY`.
    pub fn load_struct<T>(relative_path: &str) -> Result<T, anyhow::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let file_path = Self::resolve_data_path(relative_path)?;
        let json_data = fs::read_to_string(&file_path)
            .with_context(|| format!("could not read JSON data file {}", file_path.display()))?;
        serde_json::from_str(&json_data)
            .with_context(|| format!("invalid JSON in data file {}", file_path.display()))
    }

    /// Build a packet from a JSON data file.
    pub fn build_packet<T>(relative_path: &str, cmd_id: u32) -> Result<Vec<u8>, anyhow::Error>
    where
        T: serde::de::DeserializeOwned + MessageEncode,
    {
        let file_path = Self::resolve_data_path(relative_path)?;
        if !file_path.is_file() {
            return Err(anyhow::anyhow!(
                "Missing JSON file for command ID {cmd_id}: {}\nCreate this file with the message data for {}",
                file_path.display(),
                relative_path
            ));
        }

        let data: T = Self::load_struct(relative_path)?;
        Ok(build_server_packet(cmd_id, &data.encode())?)
    }

    /// Build a packet from a raw hex payload file (commands without a schema).
    pub fn load_raw_packet(relative_path: &str, cmd_id: u32) -> Result<Vec<u8>, anyhow::Error> {
        #[derive(serde::Deserialize)]
        struct RawPayload {
            payload_hex: String,
        }
        let file_path = Self::resolve_data_path(relative_path)?;
        if !file_path.is_file() {
            return Err(anyhow::anyhow!(
                "Missing raw payload file for command ID {cmd_id}: {}\nCreate it as {{\"payload_hex\": \"<hex bytes>\"}}",
                file_path.display()
            ));
        }
        let raw: RawPayload = Self::load_struct(relative_path)?;
        let bytes =
            hex_decode(&raw.payload_hex).with_context(|| format!("invalid hex in {relative_path}"))?;
        Ok(build_server_packet(cmd_id, &bytes)?)
    }

    fn resolve_data_path(relative_path: &str) -> Result<PathBuf, anyhow::Error> {
        let path = Path::new(relative_path);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
            })
        {
            anyhow::bail!("data file path must stay inside DATA_DIRECTORY: {relative_path}");
        }
        Ok(DATA_DIRECTORY.join(path))
    }

    /// Load the complete hero biography response sequence from JSON files
    pub fn load_hero_biography_sequence() -> Result<Vec<Vec<u8>>, anyhow::Error> {
        // Exact packet order of the official server's response to
        // CS_HERO_BIOGRAPHY_INFO (18006), including schema-less packets that are
        // replayed from raw hex files.  Deviating from this order (or dropping
        // packets) leaves the client stuck on the loading screen.
        let mut packets = Vec::new();

        load_raw!("hero_biography/unknown_19910.json", 19910, packets);
        load_packet!(SC_TODAY_NOT_NOTICE, "hero_biography/today_not_notice.json", 12006, packets);
        load_packet!(SC_NOVICE_TRAINING_PANEL, "hero_biography/novice_training_panel.json", 24112, packets);
        load_packet!(SC_NOVICE_TARGET_PANEL_INFO, "hero_biography/novice_target_panel.json", 24041, packets);
        load_packet!(SC_ACHIEVEMENT_PANEL_INFO, "hero_biography/achievement_panel.json", 24021, packets);
        load_packet!(SC_PLATFORM_ACHIEVE_PANEL, "hero_biography/platform_achieve_panel.json", 24276, packets);
        load_packet!(SC_FIRST_PAY_PANEL, "hero_biography/first_pay_panel.json", 24212, packets);
        load_packet!(SC_TITANIUM_EXCHANGE_GOLD_COIN_INFO, "hero_biography/titanium_exchange.json", 12055, packets);
        load_packet!(SC_MAIL_LIST, "hero_biography/mail_list.json", 16001, packets);

        // The captured client initializes eight bag types (incl. type 8).
        for bag_type in 1..=8 {
            load_packet!(SC_BAG_INIT, &format!("hero_biography/bag_init_type{}.json", bag_type), 17000, packets);
        }

        load_packet!(SC_IS_RENAME, "hero_biography/is_rename.json", 12062, packets);
        load_packet!(SC_PLAYER_HOMEPAGE_INFO, "hero_biography/player_homepage.json", 12034, packets);
        load_packet!(SC_FRIEND_LIST, "hero_biography/friend_list.json", 15001, packets);
        load_packet!(SC_BLACK_LIST, "hero_biography/black_list.json", 15013, packets);
        load_packet!(SC_FRIEND_APPLY_LIST, "hero_biography/friend_apply_list.json", 15003, packets);
        load_packet!(SC_SETTING, "hero_biography/settings.json", 10101, packets);
        load_packet!(SC_GET_SERVER_STATE, "hero_biography/server_state.json", 10071, packets);
        load_packet!(SC_MONTH_CARD_PANEL, "hero_biography/month_card_panel.json", 24095, packets);
        load_packet!(SC_STAMINA_MONTH_CARD_PANEL, "hero_biography/stamina_month_card_panel.json", 24351, packets);
        load_packet!(SC_DIRECT_GIFT_PANEL, "hero_biography/direct_gift_panel.json", 24097, packets);
        load_packet!(SC_FUND_PANEL, "hero_biography/fund_panel.json", 24206, packets);
        load_packet!(SC_FASHION_SCENE_PANEL, "hero_biography/fashion_scene_panel.json", 13370, packets);
        load_packet!(SC_FASHION_INFO, "hero_biography/fashion_info.json", 13107, packets);
        load_packet!(SC_HERO_PRE_LIST, "hero_biography/hero_pre_list.json", 13060, packets);
        load_packet!(SC_ACT_FETTER_INFO, "hero_biography/act_fetter_info.json", 13362, packets);
        load_packet!(SC_HERO_FASHION_HAVE_INFO, "hero_biography/hero_fashion_have_info.json", 13350, packets);
        load_packet!(SC_PLAYER_GUIDE_INFO, "hero_biography/player_guide_info.json", 12058, packets);
        load_packet!(SC_PLAYER_STORY_INFO, "hero_biography/player_story_info.json", 12053, packets);
        load_packet!(SC_MAIN_STORY_INFO, "hero_biography/main_story_info.json", 18000, packets);
        load_packet!(SC_MAIN_STORY_STAGE_AWARD_LIST, "hero_biography/main_story_award_list.json", 18054, packets);
        load_packet!(SC_PLAYER_STAMINA_INFO, "hero_biography/player_stamina_info.json", 12051, packets);
        load_packet!(SC_DUP_DATA, "hero_biography/dup_data.json", 18001, packets);
        load_packet!(SC_CHIP_DUP_PICK_SUIT, "hero_biography/chip_dup_pick_suit.json", 18062, packets);
        load_packet!(SC_COLLEGE_DELEGATION_HALL_INFO, "hero_biography/college_delegation.json", 19002, packets);
        load_packet!(SC_SHOP_DATA, "hero_biography/shop_data.json", 17006, packets);
        load_packet!(SC_RECRUIT_INFO, "hero_biography/recruit_info.json", 13050, packets);
        load_packet!(SC_BATTLE_REPLAY_INFOS, "hero_biography/battle_replay_info.json", 20111, packets);
        load_packet!(SC_RES_ALL_MODULE_READ, "hero_biography/res_all_module_read.json", 10056, packets);
        load_packet!(SC_GET_AVATAR_LIST, "hero_biography/get_avatar_list.json", 12018, packets);
        load_packet!(SC_GET_AVATAR_FRAME_LIST, "hero_biography/get_avatar_frame_list.json", 12020, packets);
        load_packet!(SC_GET_DIALOG_BOX_LIST, "hero_biography/get_dialog_box_list.json", 12036, packets);
        load_packet!(SC_GET_DESIGNATION_LIST, "hero_biography/get_designation_list.json", 12022, packets);
        load_packet!(SC_GET_BACKGROUND_LIST, "hero_biography/get_background_list.json", 12067, packets);
        load_packet!(SC_HERO_ACTION_LIST, "hero_biography/hero_action_list.json", 13280, packets);
        load_packet!(SC_PAY_DATA, "hero_biography/pay_data.json", 10021, packets);
        load_packet!(SC_SIGN_PANEL, "hero_biography/sign_panel.json", 24033, packets);
        load_packet!(SC_FORCES_PANEL, "hero_biography/force_panel.json", 21001, packets);
        load_packet!(SC_SEVEN_DAY_PANEL_INFO, "hero_biography/seven_day_panel.json", 24066, packets);
        load_packet!(SC_SYSTEM_ANNOUNCE, "hero_biography/system_annnounce.json", 10032, packets);
        load_packet!(SC_ACC_PAY_PANEL, "hero_biography/acc_pay_panel.json", 24106, packets);
        load_packet!(SC_HERO_LV_REWARD, "hero_biography/hero_lv_reward.json", 13200, packets);
        load_packet!(SC_RELATION_REWARD, "hero_biography/relation_reward.json", 13142, packets);
        load_packet!(SC_NEW_UNREAD, "hero_biography/new_unread.json", 10059, packets);
        load_packet!(SC_MONSTER_MANUAL, "hero_biography/monster_manual.json", 12100, packets);
        load_packet!(SC_EQUIP_SUIT_MANUAL, "hero_biography/equip_suit_manual.json", 12101, packets);
        load_packet!(SC_BRACELET_MANUAL, "hero_biography/bracelet_manual.json", 12102, packets);
        load_packet!(SC_MUSIC_MANUAL, "hero_biography/music_manual.json", 12103, packets);
        load_packet!(SC_STORY_MANUAL, "hero_biography/story_manual.json", 12104, packets);
        load_packet!(SC_WORLD_MANUAL, "hero_biography/world_manual.json", 12105, packets);
        load_raw!("hero_biography/unknown_12106.json", 12106, packets);
        load_packet!(SC_HERO_ASSIST_FIGHT_SKILL, "hero_biography/assist_fight_skill.json", 13220, packets);
        load_packet!(SC_HERO_FORMATION, "hero_biography/hero_formation.json", 13041, packets);
        load_packet!(SC_FASHION_SHOP_PANEL, "hero_biography/fashion_shop_panel.json", 24131, packets);
        load_packet!(SC_LEVEL_GIFT_PANEL, "hero_biography/level_gift_panel.json", 24201, packets);
        load_packet!(SC_DIALOGUE_PANEL, "hero_biography/dialogue_panel.json", 12160, packets);
        load_packet!(SC_ACTIVITY_NOVICE_START, "hero_biography/activity_novice_start.json", 24250, packets);
        load_packet!(SC_ACTIVITY_NOVICE_RECRUIT_HERO_PANEL, "hero_biography/activity_novice_recruit.json", 24220, packets);
        load_packet!(SC_ACTIVITY_NOVICE_UPGRADE_PANEL, "hero_biography/activity_novice_upgrade.json", 24240, packets);
        load_packet!(SC_OPEN_SERVER_SIGN_PANEL_INFO, "hero_biography/open_server_sign_panel.json", 24271, packets);
        load_packet!(SC_PARKOUR_PANEL, "hero_biography/parked_panel.json", 18120, packets);
        load_packet!(SC_GUILD_PANEL, "hero_biography/guild_panel.json", 23002, packets);
        load_packet!(SC_REFRESH_RECOMMEND_GUILDS, "hero_biography/refresh_recommend_guilds.json", 23008, packets);
        load_packet!(SC_DOWN_GIFT_SHOW, "hero_biography/down_gift_show.json", 24372, packets);
        load_packet!(SC_HERO_TRY_INFO, "hero_biography/hero_try_info.json", 19601, packets);
        load_packet!(SC_LIMITED_GIFT_PANEL, "hero_biography/limited_gift_panel.json", 24402, packets);
        load_packet!(SC_ACTIVITY_DAY_REWARD_PANEL, "hero_biography/activity_day_reward_panel.json", 24462, packets);
        load_packet!(SC_ACTIVITY_EXPIRED_GOODS, "hero_biography/activity_expired_goods.json", 24490, packets);
        load_raw!("hero_biography/unknown_12210.json", 12210, packets);
        load_raw!("hero_biography/unknown_12220.json", 12220, packets);
        load_packet!(SC_FUNCTION_OPEN_LIST, "login/function_open_list.json", 12009, packets);
        load_packet!(SC_ACTIVITY_OPEN_INFO, "hero_biography/open_activity_open_info.json", 19050, packets);
        // The official server sends the activity-open list twice in a row.
        load_packet!(SC_ACTIVITY_OPEN_INFO, "hero_biography/open_activity_open_info.json", 19050, packets);
        load_packet!(SC_FRIEND_GIFT_PANEL, "hero_biography/friend_gift_panel.json", 15028, packets);
        load_packet!(SC_PLAYER_HOMEPAGE_INFO, "hero_biography/player_homepage_info.json", 12034, packets);

        Ok(packets)
    }

    /// Load login sequence packets
    pub fn load_login_sequence(account_id: i64, server_time: i32) -> Result<Vec<Vec<u8>>, anyhow::Error> {
        let mut packets = Vec::new();

        // 1. Login response (dynamic data)
        let login_response = SC_ACCOUNT_LOGIN {
            result: 0,
            acc_id: account_id.to_string(),
            player_id: account_id.to_string(),
            is_new: 0,
            session: format!("{:032x}", rand::random::<u128>()),
            create_time: 1757756377,
        };
        packets.push(build_server_packet(11001, &login_response.encode())?);

        // 2. System ping (dynamic time)
        packets.push(build_server_packet(10001, &SC_SYS_PING { time: server_time }.encode())?);

        // 3. System date (dynamic time)
        packets.push(build_server_packet(10002, &SC_SYS_DATE {
            time: server_time,
            open_date: 1757666400,
            merge_date: 0,
        }.encode())?);

        // 4. Player base data (from JSON)
        packets.push(Self::build_packet::<SC_PLAYER_BASE_DATA>(
            "login/player_base_data.json", 12001)?);

        // 5. Player end data
        packets.push(build_server_packet(12000, &SC_PLAYER_END_DATA {}.encode())?);

        // 6. Function open list
        packets.push(Self::build_packet::<SC_FUNCTION_OPEN_LIST>(
            "login/function_open_list.json", 12009)?);

        Ok(packets)
    }

    pub fn load_shop_type_data(shop_type: i8) -> Result<Vec<Vec<u8>>, anyhow::Error> {
        let mut packets = Vec::new();

        let json_path = match shop_type {
            1 => "shop/shop_type_1.json",
            2 => "shop/shop_type_2.json",
            3 => "shop/shop_type_3.json",
            4 => "shop/shop_type_4.json",
            5 => "shop/shop_type_5.json",
            _ => "shop/shop_type_default.json",
        };

        load_packet!(SC_SHOP_TYPE_DATA, json_path, 17010, packets);
        Ok(packets)
    }

    /// Load dialogue response based on NPC ID and current dialogue state
    pub fn load_dialogue_talk(target_id: i32) -> Result<Vec<Vec<u8>>, anyhow::Error> {
        let mut packets = Vec::new();

        let json_path = match target_id {
            1110 => "dialogue/npc_1110.json",
            1111 => "dialogue/npc_1111.json",
            1112 => "dialogue/npc_1112.json",
            _ => "dialogue/npc_default.json",
        };

        load_packet!(SC_DIALOGUE_TALK, json_path, 12162, packets);
        Ok(packets)
    }

    pub fn load_hero_detail(hero_id: i32) -> Result<Vec<Vec<u8>>, anyhow::Error>{

        let mut packets = Vec::new();

        let json_path = if (1..=22).contains(&hero_id) {
            format!("hero/hero_detail_{}.json", hero_id)
        } else {
            "hero/hero_detail_default.json".to_string()
        };


        load_packet!(SC_HERO_DETAIL, &*json_path, 13011, packets);

        Ok(packets)

    }

    /// Load dialogue response with progression tracking
    /// This version can handle dialogue trees that progress based on previous choices
    pub fn load_dialogue_talk_with_state(target_id: i32, current_part: Option<i32>) -> Result<Vec<Vec<u8>>, anyhow::Error> {
        let mut packets = Vec::new();

        let json_path = match (target_id, current_part) {
            (1110, None) => "dialogue/npc_1110_start.json",
            (1110, Some(1)) => "dialogue/npc_1110_part_1.json",
            (1110, Some(2)) => "dialogue/npc_1110_part_2.json",
            (1110, Some(3)) => "dialogue/npc_1110_part_3.json",
            (1110, Some(_)) => "dialogue/npc_1110_end.json",
            _ => "dialogue/npc_default.json",
        };

        load_packet!(SC_DIALOGUE_TALK, json_path, 12162, packets);
        Ok(packets)
    }

    pub fn load_direct_gift_panel() -> Result<Vec<Vec<u8>>, anyhow::Error> {
        let mut packets = Vec::new();
        load_packet!(SC_DIRECT_GIFT_PANEL, "shop/direct_gift_panel.json", 24097, packets);
        Ok(packets)
    }
}


pub trait MessageEncode {
    fn encode(&self) -> Vec<u8>;
}

// Implement for all message structs used in the sequences
impl MessageEncode for SC_TODAY_NOT_NOTICE { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_NOVICE_TRAINING_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_NOVICE_TARGET_PANEL_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACHIEVEMENT_PANEL_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PLATFORM_ACHIEVE_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FIRST_PAY_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_TITANIUM_EXCHANGE_GOLD_COIN_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_MAIL_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FASHION_SCENE_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_NEW_UNREAD { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACTIVITY_EXPIRED_GOODS { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_BAG_INIT { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_IS_RENAME { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PLAYER_HOMEPAGE_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FRIEND_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_BLACK_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FRIEND_APPLY_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_SETTING { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_GET_SERVER_STATE { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_MONTH_CARD_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_STAMINA_MONTH_CARD_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_DIRECT_GIFT_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PLAYER_BASE_DATA { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FUNCTION_OPEN_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FUND_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FASHION_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HERO_PRE_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACT_FETTER_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HERO_FASHION_HAVE_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PLAYER_GUIDE_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PLAYER_STORY_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_MAIN_STORY_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_MAIN_STORY_STAGE_AWARD_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PLAYER_STAMINA_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_DUP_DATA { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_CHIP_DUP_PICK_SUIT { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_COLLEGE_DELEGATION_HALL_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_SHOP_DATA { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_RECRUIT_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_BATTLE_REPLAY_INFOS { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_RES_ALL_MODULE_READ { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_GET_AVATAR_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_GET_AVATAR_FRAME_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_GET_DIALOG_BOX_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_GET_DESIGNATION_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_GET_BACKGROUND_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HERO_ACTION_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PAY_DATA { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_SIGN_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FORCES_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_SEVEN_DAY_PANEL_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_SYSTEM_ANNOUNCE  { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACC_PAY_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HERO_LV_REWARD { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_RELATION_REWARD { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_MONSTER_MANUAL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_EQUIP_SUIT_MANUAL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_BRACELET_MANUAL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_MUSIC_MANUAL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_STORY_MANUAL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_WORLD_MANUAL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HERO_ASSIST_FIGHT_SKILL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HERO_FORMATION { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FASHION_SHOP_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_LEVEL_GIFT_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_DIALOGUE_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACTIVITY_NOVICE_START { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACTIVITY_NOVICE_RECRUIT_HERO_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACTIVITY_NOVICE_UPGRADE_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACTIVITY_NOVICE_TURNTABLE_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_OPEN_SERVER_SIGN_PANEL_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PARKOUR_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_GUILD_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_REFRESH_RECOMMEND_GUILDS { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_DOWN_GIFT_SHOW {fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HERO_TRY_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_LIMITED_GIFT_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACTIVITY_PAY_SIGN2_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_PACK_BAG_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HAPPY_FARM_FIELD_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HAPPY_FARM_ORDER_LIST { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACTIVITY_DAY_REWARD_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_ACTIVITY_OPEN_INFO { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_FRIEND_GIFT_PANEL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_SHOP_TYPE_DATA { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_DIALOGUE_TALK { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
impl MessageEncode for SC_HERO_DETAIL { fn encode(&self) -> Vec<u8> { Self::encode(self) } }
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_paths_cannot_escape_the_configured_root() {
        assert!(GameDataLoader::resolve_data_path("login/player.json").is_ok());
        assert!(GameDataLoader::resolve_data_path("../secrets.json").is_err());
        assert!(GameDataLoader::resolve_data_path("/tmp/secrets.json").is_err());
    }
}

fn hex_decode(text: &str) -> Result<Vec<u8>, anyhow::Error> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if text.len() % 2 != 0 {
        anyhow::bail!("odd-length hex string");
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(anyhow::Error::from))
        .collect()
}
