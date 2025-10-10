use std::sync::Arc;
use tokio::sync::Mutex;
use crate::data_loader::GameDataLoader;
use crate::messages::CS_DIALOGUE_TALK;
use crate::state::ConnectionContext;

pub async fn handle_dialogue_talk(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_DIALOGUE_TALK,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    println!("Dialogue with NPC: {}", request.target_id);

    // Get current dialogue state and advance it
    let current_part = {
        let mut connection = ctx.lock().await;
        let current = connection.get_dialogue_part(request.target_id);
        let next_part = connection.advance_dialogue(request.target_id);
        println!("NPC {} dialogue: part {} -> {}", request.target_id, current.unwrap_or(0), next_part);
        current
    };

    // Load dialogue response based on current state
    let packets = GameDataLoader::load_dialogue_talk_with_state(request.target_id, current_part)?;
    Ok(packets)
}