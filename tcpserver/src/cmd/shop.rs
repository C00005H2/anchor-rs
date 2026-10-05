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
