use std::sync::Arc;

use tokio::{
    io::AsyncWriteExt,
    net::tcp::OwnedWriteHalf,
    sync::Mutex,
};
use tracing::warn;

use crate::{
    capture_replay::{encode_captured_response, CaptureReplay, ReplayLookup},
    cmd::{account, dialogue, hero, shop, system, world},
    messages::{
        CS_ACCOUNT_LOGIN, CS_DIALOGUE_TALK, CS_DIRECT_GIFT_PANEL, CS_ENTER_WORLD, CS_HERO_DETAIL,
        CS_SHOP_TYPE_DATA, CS_SYS_PING, SC_ACCOUNT_LOGIN,
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
        17009 => {
            let request = CS_SHOP_TYPE_DATA::decode(data);
            let responses = shop::handle_shop_type_data(ctx, request).await?;
            send_responses!(writer, responses);
        }
        18006 => {
            let responses = world::handle_hero_biography(ctx).await?;
            send_responses!(writer, responses);
        }
        24096 => {
            let request = CS_DIRECT_GIFT_PANEL::decode(data);
            let responses = shop::handle_direct_gift_panel(ctx, request).await?;
            send_responses!(writer, responses);
        }
        _ => {
            tracing::warn!(cmd = cmd_id, "Unhandled command");
        }
    }

    Ok(())
}
