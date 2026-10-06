use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::data_loader::GameDataLoader;
use crate::messages::{
    CS_ATTR_PREVIEW_ALL, CS_HERO_DETAIL, CS_HERO_EVOLUTION, SC_ATTR_PREVIEW_ALL,
    SC_HERO_EVOLUTION,
};
use crate::packet::build_server_packet;
use crate::sequence::TemplateFile;
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
    connection.ready_team_id = Some(request.team_id);
    for team in &mut connection.formation {
        team.is_ready = if team.team_id == request.team_id { 1 } else { 0 };
    }
    info!(team_id = request.team_id, "Team ready state updated");
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
    connection.formation_received = true;
    let mut formation = request.formation_list;
    if let Some(team) = formation.iter().find(|team| team.is_ready == 1) {
        connection.ready_team_id = Some(team.team_id);
    }
    if let Some(ready_team_id) = connection.ready_team_id {
        for team in &mut formation {
            team.is_ready = if team.team_id == ready_team_id { 1 } else { 0 };
        }
    }
    connection.formation = formation;
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

/// Handle CS_ATTR_PREVIEW_ALL (13150): the deploy screen asks for a stat
/// preview.  The capture never recorded this flow, so echo the request and
/// report no extra attributes.
pub async fn handle_attr_preview_all(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_ATTR_PREVIEW_ALL,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    info!(
        hero_id = request.hero_id,
        module_id = request.module_id,
        "Attribute preview requested"
    );

    // Prefer the captured preview for this hero/module pair when available.
    let path = format!(
        "hero/attr_preview_{}_{}.json",
        request.hero_id, request.module_id
    );
    if let Ok(script) = TemplateFile::load(&path) {
        if let Some(group) = script.first_group().cloned() {
            let cursor = ctx.lock().await.replay_cursor.clone();
            return group.encode(&cursor);
        }
    }

    Ok(vec![build_server_packet(
        13151,
        &SC_ATTR_PREVIEW_ALL {
            hero_id: request.hero_id,
            module_id: request.module_id,
            param_int: request.param_int,
            attr_preview: Vec::new(),
        }
        .encode(),
    )?])
}



/// Handle CS_HERO_EVOLUTION (13004): accept the evolution and report the new
/// evolution level.  Levels are tracked per hero instance per connection.
pub async fn handle_hero_evolution(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_HERO_EVOLUTION,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let evolution = {
        let mut connection = ctx.lock().await;
        let entry = connection.hero_evolution.entry(request.id).or_insert(0);
        *entry += 1;
        *entry
    };
    info!(hero_id = request.id, evolution = evolution, "Hero evolved");
    Ok(vec![build_server_packet(
        13005,
        &SC_HERO_EVOLUTION {
            result: 1,
            id: request.id,
            evolution,
        }
        .encode(),
    )?])
}
