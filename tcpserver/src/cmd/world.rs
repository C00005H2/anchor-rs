use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::data_loader::GameDataLoader;
use crate::messages::{
    CS_ENTER_WORLD, SC_HERO_FORMATION, SC_MAIN_STORY_INFO, SC_MAIN_STORY_STAGE_AWARD_LIST,
};
use crate::packet::build_server_packet;
use crate::state::ConnectionContext;

/// Handle the world-entry request.
pub async fn handle_enter_world(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_ENTER_WORLD,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!(battle_sync_word = request.battle_sync_word, "Player entering world");

    let mut connection = ctx.lock().await;
    if !connection.is_authenticated() {
        tracing::warn!("Rejecting world entry before account login");
        return Ok(Vec::new());
    }
    connection.logged_in = true;
    Ok(Vec::new())
}

/// Handle homepage info request. Response data is currently supplied by the
/// game initialization sequence, so this command is intentionally empty.
pub async fn handle_homepage_info(
    _ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!("Player requested homepage info");
    Ok(Vec::new())
}

/// Load the configured hero biography initialization sequence, patched with
/// live connection state for story progression, deployed formation, and claimed awards.
pub async fn handle_hero_biography(
    ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!("Player requested hero biography info");
    let mut packets = GameDataLoader::load_hero_biography_sequence()?;

    let (story_info, formation, awards) = {
        let connection = ctx.lock().await;
        (
            SC_MAIN_STORY_INFO {
                now_stage_list: connection.story_now_stage_list.clone(),
                pass_stage_list: connection.story_pass_stage_list.clone(),
                ongoing_stage_id: 0,
                play_chapter_pic_list: connection.story_play_chapter_pic_list.clone(),
            },
            connection.formation.clone(),
            connection.story_award_claimed.clone(),
        )
    };

    if let Ok(story_pkt) = build_server_packet(18000, &story_info.encode()) {
        for pkt in &mut packets {
            if pkt.len() >= 6 && u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]) == 18000 {
                *pkt = story_pkt;
                break;
            }
        }
    }

    if !formation.is_empty() {
        let formation_msg = SC_HERO_FORMATION {
            msg_type: 1,
            formation_list: formation,
        };
        if let Ok(form_pkt) = build_server_packet(13041, &formation_msg.encode()) {
            for pkt in &mut packets {
                if pkt.len() >= 6 && u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]) == 13041 {
                    *pkt = form_pkt;
                    break;
                }
            }
        }
    }

    if !awards.is_empty() {
        let award_msg = SC_MAIN_STORY_STAGE_AWARD_LIST {
            stage_list: awards,
        };
        if let Ok(award_pkt) = build_server_packet(18054, &award_msg.encode()) {
            for pkt in &mut packets {
                if pkt.len() >= 6 && u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]) == 18054 {
                    *pkt = award_pkt;
                    break;
                }
            }
        }
    }

    Ok(packets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handle_hero_biography_patches_live_story_and_formation() {
        let mut connection = ConnectionContext::new("test_biography".to_owned());
        connection.story_now_stage_list = vec![1008];
        connection.story_pass_stage_list = vec![1007, 1006, 1005, 1004, 1003, 1002, 1001];
        connection.story_award_claimed = vec![1001, 1002];
        let ctx = Arc::new(Mutex::new(connection));

        let packets = handle_hero_biography(ctx).await.expect("load biography");
        let mut found_story = false;
        let mut found_awards = false;

        for pkt in &packets {
            assert!(pkt.len() >= 6);
            let cmd = u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]);
            if cmd == 18000 {
                let decoded = SC_MAIN_STORY_INFO::decode(&pkt[6..]);
                assert_eq!(decoded.now_stage_list, vec![1008]);
                assert_eq!(decoded.pass_stage_list.len(), 7);
                found_story = true;
            } else if cmd == 18054 {
                let decoded = SC_MAIN_STORY_STAGE_AWARD_LIST::decode(&pkt[6..]);
                assert_eq!(decoded.stage_list, vec![1001, 1002]);
                found_awards = true;
            }
        }

        assert!(found_story, "SC_MAIN_STORY_INFO must be present and patched");
        assert!(found_awards, "SC_MAIN_STORY_STAGE_AWARD_LIST must be present and patched");
    }
}
