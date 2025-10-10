use std::sync::Arc;
use tokio::sync::Mutex;
use crate::data_loader::GameDataLoader;
use crate::messages::{CS_DIRECT_GIFT_PANEL, CS_SHOP_TYPE_DATA};
use crate::state::ConnectionContext;

pub async fn handle_shop_type_data(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_SHOP_TYPE_DATA,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let packets = GameDataLoader::load_shop_type_data(request.shop_type)?;
    Ok(packets)
}

pub async fn handle_direct_gift_panel(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_DIRECT_GIFT_PANEL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    println!("Loading direct gift panel");

    let packets = GameDataLoader::load_direct_gift_panel()?;
    Ok(packets)
}