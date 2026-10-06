use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::data_loader::GameDataLoader;
use crate::messages::{CS_DIRECT_GIFT_PANEL, CS_SHOP_TYPE_DATA};
use crate::state::ConnectionContext;

pub async fn handle_shop_type_data(
    _ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_SHOP_TYPE_DATA,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    GameDataLoader::load_shop_type_data(request.shop_type)
}

pub async fn handle_direct_gift_panel(
    _ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_DIRECT_GIFT_PANEL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!("Loading direct gift panel");
    GameDataLoader::load_direct_gift_panel()
}

use crate::messages::{
    pt_attr_int, CS_DIRECT_GIFT_BUY, CS_SHOP_BUY, SC_DIRECT_GIFT_BUY, SC_DIRECT_GIFT_PANEL,
    SC_SHOP_BUY,
};
use crate::packet::build_server_packet;
use crate::progression::{grant_rewards, GiftTable};

const GIFT_TABLE_DATA: &str = "progression/direct_gift.json";
const GIFT_PANEL_DATA: &str = "shop/direct_gift_panel.json";

/// Handle CS_DIRECT_GIFT_BUY (24098).
///
/// Purchases are limited to once per goods id per session (the captured goods
/// are single-purchase).  The reply updates attributes, grants the awards and
/// refreshes the panel including the new buy list.
pub async fn handle_direct_gift_buy(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_DIRECT_GIFT_BUY,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let table: GiftTable = GameDataLoader::load_struct(GIFT_TABLE_DATA)?;
    let goods = table
        .goods
        .iter()
        .find(|entry| entry.goods_id == request.goods_id)
        .cloned();

    let mut packets = Vec::new();
    {
        let mut connection = ctx.lock().await;
        match goods {
            Some(goods) if connection.claim_once("direct_gift", i64::from(request.goods_id)) => {
                packets.extend(grant_rewards(&mut connection, &goods.reward)?);
                packets.push(build_server_packet(
                    24099,
                    &SC_DIRECT_GIFT_BUY {
                        goods_id: request.goods_id,
                        num: request.num,
                        award_list: goods.reward.award_list.clone(),
                    }
                    .encode(),
                )?);
                connection.record_purchase(request.goods_id, request.num);
                info!(goods_id = request.goods_id, num = request.num, "Direct gift purchased");
            }
            _ => {
                info!(goods_id = request.goods_id, "Direct gift purchase rejected");
            }
        }

        // Refreshed panel with the session's buy list.
        let mut panel: SC_DIRECT_GIFT_PANEL = GameDataLoader::load_struct(GIFT_PANEL_DATA)?;
        panel.buy_list = connection
            .claimed_ids("direct_gift")
            .iter()
            .map(|goods_id| pt_attr_int {
                key: *goods_id as i16,
                value: i32::from(connection.purchase_count(*goods_id as i32)),
            })
            .collect();
        panel.request_times = connection.bump_request_count("direct_gift");
        packets.push(build_server_packet(24097, &panel.encode())?);
    }
    Ok(packets)
}


/// Handle CS_SHOP_BUY (17007): acknowledge the purchase.  Captures contain no
/// shop purchases, so the buy is confirmed without awarding specific items —
/// the alternative (silence) leaves the client hanging on the shop screen.
pub async fn handle_shop_buy(
    _ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_SHOP_BUY,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    tracing::info!(
        shop_type = request.shop_type,
        shop_id = request.shop_id,
        num = request.num,
        "Shop purchase acknowledged"
    );
    Ok(vec![build_server_packet(
        17008,
        &SC_SHOP_BUY {
            shop_type: request.shop_type,
            shop_id: request.shop_id,
            num: request.num,
            award_list: Vec::new(),
        }
        .encode(),
    )?])
}
