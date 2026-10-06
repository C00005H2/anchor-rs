use std::sync::Arc;

use tokio::{
    io::AsyncWriteExt,
    net::tcp::OwnedWriteHalf,
    sync::Mutex,
};
use tracing::warn;

use crate::{
    capture_replay::{encode_captured_response, CaptureReplay, ReplayLookup},
    cmd::{account, activity, battle, dialogue, hero, mail, shop, system, world},
    messages::{
        CS_ACCOUNT_LOGIN, CS_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE, CS_ATTR_PREVIEW_ALL,
        CS_BATTLE_AUTO,
        CS_BATTLE_FIELD_ENTER, CS_BATTLE_START, CS_BATTLE_VIDEO_END, CS_CANNOT_DEL_HERO_LIST,
        CS_CHANGE_HERO, CS_DIALOGUE_TALK, CS_DIRECT_GIFT_BUY, CS_DIRECT_GIFT_PANEL,
        CS_ENTER_WORLD, CS_GAIN_ACHIEVEMENT_AWARD, CS_GAIN_OPEN_SERVER_SIGN_REWARD,
        CS_GAIN_SEVEN_DAY_REWARD, CS_HERO_DETAIL, CS_MAIL_ENCLOSURE_REC, CS_MAIL_READ,
        CS_NOVICE_TRAINING_PANEL, CS_NOVICE_TRAINING_RECEIVE_TASK, CS_PUBLIC_CHAT_SETTING,
        CS_RECRUIT_HERO_NEW_SAVE_LIST, CS_REQ_MODULE_READ, CS_SET_READY, CS_SHOP_TYPE_DATA,
        CS_SYS_PING, SC_ACCOUNT_LOGIN,
    },
    state::ConnectionContext,
};

#[macro_export]
macro_rules! send_responses {
    ($writer:expr, $responses:expr) => {
        for pkt in $responses {
            if pkt.len() >= 6 {
                let cmd_id = u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]);
                let body = &pkt[6..];

                let name = crate::msgid::MsgId::try_from(cmd_id)
                    .map(|id| id.to_string())
                    .unwrap_or_else(|_| format!("UNKNOWN({})", cmd_id));

                tracing::info!(
                    cmd = cmd_id,
                    name = %name,
                    payload_len = body.len(),
                    "Sending server packet"
                );
                if matches!(cmd_id, 11001 | 11008) {
                    tracing::debug!("Omitting raw authentication response preview");
                } else {
                    tracing::debug!(preview = %crate::packet::hex_preview(body, 32), "Server packet preview");
                }

                if let Some(mut val) = crate::dispatch::dispatch_cmd(cmd_id, body) {
                    crate::capture_replay::redact_capture_value(cmd_id, &mut val);
                    tracing::debug!(decoded = %val, "Decoded server packet");
                }
            }

            $writer.write_all(&pkt).await?;
        }
        $writer.flush().await?;
    };
}


/// Main packet dispatcher - handles all incoming commands.
pub async fn dispatch_packet(
    ctx: Arc<Mutex<ConnectionContext>>,
    cmd_id: u32,
    data: &[u8],
    writer: &mut OwnedWriteHalf,
) -> Result<(), anyhow::Error> {
    dispatch_packet_with_replay(ctx, cmd_id, data, writer, None).await
}

/// Dispatch a request using a matching captured response group when replay is
/// enabled, otherwise fall back to the normal command handlers.
pub async fn dispatch_packet_with_replay(
    ctx: Arc<Mutex<ConnectionContext>>,
    cmd_id: u32,
    data: &[u8],
    writer: &mut OwnedWriteHalf,
    replay_archive: Option<&CaptureReplay>,
) -> Result<(), anyhow::Error> {
    if let Some(archive) = replay_archive {
        let lookup = {
            let mut connection = ctx.lock().await;
            archive.next_group(&mut connection.replay_cursor, cmd_id)
        };

        match lookup {
            ReplayLookup::NotCaptured => {}
            ReplayLookup::Exhausted => {
                warn!(command = cmd_id, "Capture replay entries exhausted for command");
                return Ok(());
            }
            ReplayLookup::Group(group) => {
                let cursor = { ctx.lock().await.replay_cursor.clone() };
                let mut packets = Vec::with_capacity(group.responses.len());

                for captured in group.responses {
                    let decoded = cursor.rehydrate(&captured.decoded);
                    if captured.cmd == 11001 {
                        if let Ok(login) = serde_json::from_value::<SC_ACCOUNT_LOGIN>(decoded.clone()) {
                            let mut connection = ctx.lock().await;
                            if login.result == 0 {
                                connection.logged_in = true;
                                connection.player_id = login.player_id.parse().ok();
                                connection.session_id = login.session;
                                connection.update_heartbeat();
                            }
                        }
                    }

                    let response = crate::capture_replay::CapturedResponse {
                        cmd: captured.cmd,
                        decoded,
                        raw: captured.raw,
                    };
                    match encode_captured_response(&response) {
                        Ok(Some(packet)) => packets.push(packet),
                        Ok(None) => warn!(
                            command = captured.cmd,
                            "Skipping captured response with no known message schema"
                        ),
                        Err(error) => warn!(
                            command = captured.cmd,
                            error = %error,
                            "Could not encode captured response; continuing with remaining packets"
                        ),
                    }
                }

                send_responses!(writer, packets);
                return Ok(());
            }
        }
    }

    match cmd_id {
        10000 => {
            let request = CS_SYS_PING::decode(data);
            let responses = system::handle_ping(ctx, request).await?;
            send_responses!(writer, responses);
        }
        10054 => {
            let request = CS_PUBLIC_CHAT_SETTING::decode(data);
            let responses = system::handle_public_chat_setting(ctx, request).await?;
            send_responses!(writer, responses);
        }
        10057 => {
            let request = CS_REQ_MODULE_READ::decode(data);
            let responses = system::handle_req_module_read(ctx, request).await?;
            send_responses!(writer, responses);
        }
        11000 => {
            let request = CS_ACCOUNT_LOGIN::decode(data);
            let responses = account::handle_account_login(ctx, request).await?;
            send_responses!(writer, responses);
        }
        11005 => {
            let request = CS_ENTER_WORLD::decode(data);
            let responses = world::handle_enter_world(ctx, request).await?;
            send_responses!(writer, responses);
        }
        12033 => {
            let responses = world::handle_homepage_info(ctx).await?;
            send_responses!(writer, responses);
        }
        12161 => {
            let request = CS_DIALOGUE_TALK::decode(data);
            let responses = dialogue::handle_dialogue_talk(ctx, request).await?;
            send_responses!(writer, responses);
        }
        13010 => {
            let request = CS_HERO_DETAIL::decode(data);
            let responses = hero::handle_hero_detail(ctx, request).await?;
            send_responses!(writer, responses);
        }
        13044 => {
            let request = CS_SET_READY::decode(data);
            let responses = hero::handle_set_ready(ctx, request).await?;
            send_responses!(writer, responses);
        }
        13046 => {
            let request = CS_CHANGE_HERO::decode(data);
            let responses = hero::handle_change_hero(ctx, request).await?;
            send_responses!(writer, responses);
        }
        13061 => {
            let request = CS_CANNOT_DEL_HERO_LIST::decode(data);
            let responses = hero::handle_cannot_del_hero_list(ctx, request).await?;
            send_responses!(writer, responses);
        }
        13150 => {
            let request = CS_ATTR_PREVIEW_ALL::decode(data);
            let responses = hero::handle_attr_preview_all(ctx, request).await?;
            send_responses!(writer, responses);
        }
        13290 => {
            let request = CS_RECRUIT_HERO_NEW_SAVE_LIST::decode(data);
            let responses = hero::handle_recruit_hero_new_save_list(ctx, request).await?;
            send_responses!(writer, responses);
        }
        16005 => {
            let request = CS_MAIL_READ::decode(data);
            let responses = mail::handle_mail_read(ctx, request).await?;
            send_responses!(writer, responses);
        }
        16007 => {
            let request = CS_MAIL_ENCLOSURE_REC::decode(data);
            let responses = mail::handle_mail_enclosure_rec(ctx, request).await?;
            send_responses!(writer, responses);
        }
        17009 => {
            let request = CS_SHOP_TYPE_DATA::decode(data);
            let responses = shop::handle_shop_type_data(ctx, request).await?;
            send_responses!(writer, responses);
        }
        18006 => {
            let responses = world::handle_hero_biography(ctx).await?;
            send_responses!(writer, responses);
        }
        20100 => {
            let request = CS_BATTLE_FIELD_ENTER::decode(data);
            let responses = battle::handle_battle_field_enter(ctx, request).await?;
            send_responses!(writer, responses);
        }
        20102 => {
            let request = CS_BATTLE_START::decode(data);
            let responses = battle::handle_battle_start(ctx, request).await?;
            send_responses!(writer, responses);
        }
        20104 => {
            let request = CS_BATTLE_VIDEO_END::decode(data);
            let responses = battle::handle_battle_video_end(ctx, request).await?;
            send_responses!(writer, responses);
        }
        20113 => {
            let request = CS_BATTLE_AUTO::decode(data);
            let responses = battle::handle_battle_auto(ctx, request).await?;
            send_responses!(writer, responses);
        }
        24022 => {
            let request = CS_GAIN_ACHIEVEMENT_AWARD::decode(data);
            let responses = activity::handle_gain_achievement_award(ctx, request).await?;
            send_responses!(writer, responses);
        }
        24065 => {
            let request = CS_GAIN_SEVEN_DAY_REWARD::decode(data);
            let responses = activity::handle_gain_seven_day_reward(ctx, request).await?;
            send_responses!(writer, responses);
        }
        24096 => {
            let request = CS_DIRECT_GIFT_PANEL::decode(data);
            let responses = shop::handle_direct_gift_panel(ctx, request).await?;
            send_responses!(writer, responses);
        }
        24098 => {
            let request = CS_DIRECT_GIFT_BUY::decode(data);
            let responses = shop::handle_direct_gift_buy(ctx, request).await?;
            send_responses!(writer, responses);
        }
        24111 => {
            let request = CS_NOVICE_TRAINING_PANEL::decode(data);
            let responses = activity::handle_novice_training_panel(ctx, request).await?;
            send_responses!(writer, responses);
        }
        24113 => {
            let request = CS_NOVICE_TRAINING_RECEIVE_TASK::decode(data);
            let responses = activity::handle_novice_training_receive_task(ctx, request).await?;
            send_responses!(writer, responses);
        }
        24221 => {
            let request = CS_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE::decode(data);
            let responses = activity::handle_novice_recruit_receive(ctx, request).await?;
            send_responses!(writer, responses);
        }
        24270 => {
            let request = CS_GAIN_OPEN_SERVER_SIGN_REWARD::decode(data);
            let responses = activity::handle_gain_open_server_sign_reward(ctx, request).await?;
            send_responses!(writer, responses);
        }
        _ => {
            tracing::warn!(cmd = cmd_id, "Unhandled command");
        }
    }

    Ok(())
}
