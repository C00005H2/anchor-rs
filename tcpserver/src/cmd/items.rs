//! Bag item usage replayed from the captured flow.
//!
//! Each recorded `CS_USE_BY_ID` group (stamina potions, gift boxes, ...) is
//! stored with the item id that produced it; replays match by id so opening a
//! gift box sends the recorded gift-box flow rather than the potion flow.

use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::info;

use crate::{
    cmd::battle::absorb_attr_updates,
    data_loader::GameDataLoader,
    messages::{CS_USE_BY_ID, SC_BAG_UPDATE},
    packet::build_server_packet,
    sequence::{TemplateGroup, TemplateResponse},
    state::ConnectionContext,
};

const USE_BY_ID_DATA: &str = "items/use_by_id.json";

#[derive(Clone, Debug, Default, Deserialize)]
struct UseRequest {
    #[serde(default)]
    id: Option<i32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct UseEntry {
    #[serde(default)]
    request: Option<UseRequest>,
    #[serde(default)]
    responses: Vec<TemplateResponse>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct UseFile {
    #[serde(default)]
    groups: Vec<UseEntry>,
}

/// Handle CS_USE_BY_ID (17002): replay the recorded consumption flow
/// (stamina/bag updates, gift-box awards and module-read notices).
///
/// The recorded group for the requested item id wins; otherwise the first
/// recording is used.  Without any capture data the item is simply consumed
/// (removed from the bag) so the client does not hang.
pub async fn handle_use_by_id(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_USE_BY_ID,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let file: UseFile = GameDataLoader::load_struct(USE_BY_ID_DATA)?;
    let matched = file
        .groups
        .iter()
        .find(|entry| entry.request.as_ref().and_then(|req| req.id) == Some(request.id))
        .or_else(|| file.groups.first());

    let Some(entry) = matched else {
        return consume_fallback(request.id);
    };

    let cursor = ctx.lock().await.replay_cursor.clone();
    info!(item_id = request.id, count = request.count, "Item used (scripted replay)");
    let group = TemplateGroup {
        responses: entry.responses.clone(),
    };
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, &group);
    }
    Ok(packets)
}

/// No recorded flow for this item: acknowledge by removing it from the bag.
fn consume_fallback(item_id: i32) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    Ok(vec![build_server_packet(
        17001,
        &SC_BAG_UPDATE {
            msg_type: 1,
            updateList: Vec::new(),
            delList: vec![item_id],
        }
        .encode(),
    )?])
}
