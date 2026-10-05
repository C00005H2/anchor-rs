use std::sync::Arc;
use tokio::sync::Mutex;

use crate::data_loader::GameDataLoader;
use crate::messages::CS_DIALOGUE_TALK;
use crate::state::ConnectionContext;

pub async fn handle_dialogue_talk(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_DIALOGUE_TALK,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let target_id = request.target_id;
    let current_part = {
        let connection = ctx.lock().await;
        connection.get_dialogue_part(target_id)
    };

    // Do not advance progression until the response has been loaded and can be
    // sent; a missing or invalid data file must not silently skip dialogue.
    let packets = GameDataLoader::load_dialogue_talk_with_state(target_id, current_part)?;
    {
        let mut connection = ctx.lock().await;
        let next_part = connection.advance_dialogue(target_id);
        tracing::debug!(
            target_id = target_id,
            current_part = ?current_part,
            next_part = next_part,
            "Advanced dialogue state"
        );
    }
    Ok(packets)
}
