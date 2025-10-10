use std::sync::Arc;
use tokio::sync::Mutex;
use crate::messages::{CS_ACCOUNT_LOGIN, CS_SYS_PING, CS_ENTER_WORLD, CS_SHOP_TYPE_DATA, CS_DIALOGUE_TALK, CS_DIRECT_GIFT_PANEL, CS_HERO_DETAIL};
use crate::state::ConnectionContext;
use crate::cmd::{account, dialogue, hero, shop, system, world};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::io::AsyncWriteExt;

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
                    "[S->C] cmd={} ({}) payload_len={} preview={}",
                    cmd_id,
                    name,
                    body.len(),
                    crate::packet::hex_preview(body, 32),
                );

                if let Some(val) = crate::dispatch::dispatch_cmd(cmd_id, body) {
                    tracing::info!("[DECODED] {}", val);
                }
            }

            $writer.write_all(&pkt).await?;
        }
        $writer.flush().await?;
    };
}


/// Main packet dispatcher - handles all incoming commands
pub async fn dispatch_packet(
    ctx: Arc<Mutex<ConnectionContext>>,
    cmd_id: u32,
    data: &[u8],
    writer: &mut OwnedWriteHalf,
) -> Result<(), anyhow::Error> {
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
            eprintln!("Unhandled command: {}", cmd_id);
        }
    }

    Ok(())
}
