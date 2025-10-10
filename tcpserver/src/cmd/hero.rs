use std::sync::Arc;
use tokio::sync::Mutex;
use crate::data_loader::GameDataLoader;
use crate::messages::CS_HERO_DETAIL;
use crate::state::ConnectionContext;

pub async fn handle_hero_detail(
    _ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_HERO_DETAIL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    println!("Player requesting hero detail info for {}", request.id);

    let packets =  GameDataLoader::load_hero_detail(request.id)?;
    Ok(packets)
}