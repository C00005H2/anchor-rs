//! Bag item usage replayed from the captured flow.

use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use crate::{
    cmd::battle::absorb_attr_updates,
    messages::CS_USE_BY_ID,
    sequence::TemplateFile,
    state::ConnectionContext,
};

const USE_BY_ID_DATA: &str = "items/use_by_id.json";

/// Handle CS_USE_BY_ID (17002): replay the recorded consumption flow
/// (stamina/bag updates and module-read notices).
pub async fn handle_use_by_id(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_USE_BY_ID,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(USE_BY_ID_DATA)?;
    let Some(group) = script.first_group().cloned() else {
        return Ok(Vec::new());
    };

    let cursor = ctx.lock().await.replay_cursor.clone();
    info!(item_id = request.id, count = request.count, "Item used (scripted replay)");
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, &group);
    }
    Ok(packets)
}
