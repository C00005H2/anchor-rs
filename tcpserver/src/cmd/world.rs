use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::data_loader::GameDataLoader;
use crate::messages::CS_ENTER_WORLD;
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

/// Load the configured hero biography initialization sequence.
pub async fn handle_hero_biography(
    _ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!("Player requested hero biography info");
    GameDataLoader::load_hero_biography_sequence()
}
