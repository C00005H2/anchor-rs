use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::data_loader::GameDataLoader;
use crate::messages::CS_HERO_DETAIL;
use crate::state::ConnectionContext;

pub async fn handle_hero_detail(
    _ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_HERO_DETAIL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!(hero_id = request.id, "Loading hero detail");
    GameDataLoader::load_hero_detail(request.id)
}

use crate::messages::{
    CS_CANNOT_DEL_HERO_LIST, CS_CHANGE_HERO, CS_SET_READY,
};

/// Handle CS_SET_READY (13044): the real server sent no reply.
pub async fn handle_set_ready(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_SET_READY,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut connection = ctx.lock().await;
    connection.update_heartbeat();
    info!(team_id = request.team_id, "Team ready state noted");
    Ok(Vec::new())
}

/// Handle CS_CHANGE_HERO (13046): store the formation for battle entry.
pub async fn handle_change_hero(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_CHANGE_HERO,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut connection = ctx.lock().await;
    connection.update_heartbeat();
    info!(formations = request.formation_list.len(), "Hero formation updated");
    connection.formation = request.formation_list;
    Ok(Vec::new())
}

/// Handle CS_CANNOT_DEL_HERO_LIST (13061): the real server sent no reply.
pub async fn handle_cannot_del_hero_list(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_CANNOT_DEL_HERO_LIST,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    Ok(Vec::new())
}
