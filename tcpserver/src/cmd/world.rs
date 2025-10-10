use std::sync::Arc;
use tokio::sync::Mutex;
use crate::data_loader::GameDataLoader;
use crate::messages::CS_ENTER_WORLD;
use crate::state::ConnectionContext;

/// Handle world entry request -> No immediate response
pub async fn handle_enter_world(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_ENTER_WORLD,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    println!("Player entering world, battle_sync_word: {}", request.battle_sync_word);

    // No immediate responses, keep connection state only
    {
        let mut connection = ctx.lock().await;
        connection.logged_in = true;
    }

    Ok(vec![]) // explicitly empty response
}

/// Handle homepage info request -> Empty response for now
pub async fn handle_homepage_info(
    _ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    println!("Player requesting homepage info");

    // Could push SC_PLAYER_HOMEPAGE_INFO here once JSON is ready
    Ok(vec![])
}

/// Handle hero biography request -> Load JSON sequence
pub async fn handle_hero_biography(
    _ctx: Arc<Mutex<ConnectionContext>>,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    println!("Player requesting hero biography info");

    GameDataLoader::load_hero_biography_sequence()
}
