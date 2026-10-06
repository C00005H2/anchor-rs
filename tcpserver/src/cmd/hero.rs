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
    CS_CANNOT_DEL_HERO_LIST, CS_CHANGE_FORMATION, CS_CHANGE_HERO,
    CS_GET_STORY_BATTLE_SUPPORT_HERO_LIST, CS_HERO_FORMATION, CS_RENAME_FORMATION,
    CS_SET_READY, CS_SET_STORY_BATTLE_SUPPORT_HERO_LIST, SC_CANNOT_DEL_HERO_LIST,
    SC_CHANGE_FORMATION, SC_CHANGE_HERO, SC_GET_STORY_BATTLE_SUPPORT_HERO_LIST,
    SC_HERO_FORMATION, SC_RENAME_FORMATION, SC_SET_READY,
    SC_SET_STORY_BATTLE_SUPPORT_HERO_LIST, pt_cannot_del_hero, pt_hero_formation,
};

/// Heroes of the ready team, most-recently-deployed first like the official
/// server, so the client locks them against deletion and retirement.
fn cannot_del_list(
    formation: &[pt_hero_formation],
    ready_team_id: Option<i16>,
) -> Vec<pt_cannot_del_hero> {
    let team = ready_team_id
        .and_then(|team_id| formation.iter().find(|team| team.team_id == team_id))
        .or_else(|| formation.iter().find(|team| team.is_ready == 1))
        .or_else(|| formation.iter().find(|team| team.team_id == 1001))
        .or_else(|| {
            formation
                .iter()
                .find(|team| !team.formation_hero_list.is_empty())
        });
    team.map(|team| {
        team.formation_hero_list
            .iter()
            .filter(|hero| hero.hero_id != 0)
            .rev()
            .map(|hero| pt_cannot_del_hero {
                hero_id: hero.hero_id,
                reason: 1,
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Handle CS_SET_READY (13044): confirm the ready team.
///
/// The official replies arrive ~160ms late (the proxy groups them with the
/// next client request, which is why captures show no immediate reply), but
/// the deploy screen needs the acknowledgement to confirm the selection.
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
    Ok(vec![build_server_packet(
        13045,
        &SC_SET_READY {
            msg_type: request.msg_type,
            result: 1,
            team_id: request.team_id,
        }
        .encode(),
    )?])
}

/// Handle CS_CHANGE_HERO (13046): store the formation for battle entry and
/// acknowledge it. Without SC_CHANGE_HERO + SC_CANNOT_DEL_HERO_LIST the deploy
/// screen never confirms a placement, so deploying any character appears to do
/// nothing.
pub async fn handle_change_hero(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_CHANGE_HERO,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let msg_type = request.msg_type;
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
    connection.formation = formation.clone();
    let ready_team_id = connection.ready_team_id;
    let change = build_server_packet(
        13047,
        &SC_CHANGE_HERO {
            msg_type,
            result: 1,
            formation_list: formation,
        }
        .encode(),
    )?;
    let cannot_del = build_server_packet(
        13062,
        &SC_CANNOT_DEL_HERO_LIST {
            cannot_del_hero_list: cannot_del_list(&connection.formation, ready_team_id),
        }
        .encode(),
    )?;
    Ok(vec![change, cannot_del])
}

/// Handle CS_CANNOT_DEL_HERO_LIST (13061): report the deployed heroes so the
/// client locks them. See `handle_set_ready` for why an immediate reply is
/// needed even though captures group it with a later request.
pub async fn handle_cannot_del_hero_list(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_CANNOT_DEL_HERO_LIST,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut connection = ctx.lock().await;
    connection.update_heartbeat();
    let list = cannot_del_list(&connection.formation, connection.ready_team_id);
    Ok(vec![build_server_packet(
        13062,
        &SC_CANNOT_DEL_HERO_LIST {
            cannot_del_hero_list: list,
        }
        .encode(),
    )?])
}

/// Handle CS_HERO_FORMATION (13040): return the stored formation list, falling
/// back to the captured biography snapshot when the client never sent one.
pub async fn handle_hero_formation(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_HERO_FORMATION,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut connection = ctx.lock().await;
    connection.update_heartbeat();
    let formation = connection.formation.clone();
    if formation.is_empty() {
        return Ok(vec![GameDataLoader::build_packet::<SC_HERO_FORMATION>(
            "hero_biography/hero_formation.json",
            13041,
        )?]);
    }
    info!(teams = formation.len(), "Hero formation requested");
    Ok(vec![build_server_packet(
        13041,
        &SC_HERO_FORMATION {
            msg_type: request.msg_type,
            formation_list: formation,
        }
        .encode(),
    )?])
}

/// Handle CS_CHANGE_FORMATION (13042): store the new formation shape for the
/// team and acknowledge it, so the deploy screen confirms the change.
pub async fn handle_change_formation(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_CHANGE_FORMATION,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    {
        let mut connection = ctx.lock().await;
        connection.update_heartbeat();
        match connection
            .formation
            .iter_mut()
            .find(|team| team.team_id == request.team_id)
        {
            Some(team) => team.formation_id = request.formation_id,
            None => connection.formation.push(pt_hero_formation {
                team_id: request.team_id,
                formation_id: request.formation_id,
                is_ready: 0,
                name: String::new(),
                formation_hero_list: Vec::new(),
                assist_fight_list: Vec::new(),
                pet_id: 0,
            }),
        }
    }
    info!(
        team_id = request.team_id,
        formation_id = request.formation_id,
        "Formation shape updated"
    );
    Ok(vec![build_server_packet(
        13043,
        &SC_CHANGE_FORMATION {
            msg_type: request.msg_type,
            result: 1,
            team_id: request.team_id,
            formation_id: request.formation_id,
        }
        .encode(),
    )?])
}

/// Handle CS_RENAME_FORMATION (13048): store the new team name.
pub async fn handle_rename_formation(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_RENAME_FORMATION,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let CS_RENAME_FORMATION {
        msg_type,
        team_id,
        name,
    } = request;
    {
        let mut connection = ctx.lock().await;
        connection.update_heartbeat();
        match connection
            .formation
            .iter_mut()
            .find(|team| team.team_id == team_id)
        {
            Some(team) => team.name = name.clone(),
            None => connection.formation.push(pt_hero_formation {
                team_id,
                formation_id: 1,
                is_ready: 0,
                name: name.clone(),
                formation_hero_list: Vec::new(),
                assist_fight_list: Vec::new(),
                pet_id: 0,
            }),
        }
    }
    info!(team_id = team_id, name = %name, "Formation renamed");
    Ok(vec![build_server_packet(
        13049,
        &SC_RENAME_FORMATION {
            msg_type,
            team_id,
            name,
            result: 1,
        }
        .encode(),
    )?])
}

/// Handle CS_SET_STORY_BATTLE_SUPPORT_HERO_LIST (13092): accept the chosen
/// support heroes. The emulator never recorded this flow, so the choice is
/// acknowledged without being stored.
pub async fn handle_set_story_battle_support(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_SET_STORY_BATTLE_SUPPORT_HERO_LIST,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    info!(
        dup_type = request.dup_type,
        dup_id = request.dup_id,
        supports = request.support_list.len(),
        "Story battle support heroes set"
    );
    Ok(vec![build_server_packet(
        13093,
        &SC_SET_STORY_BATTLE_SUPPORT_HERO_LIST { result: 1 }.encode(),
    )?])
}

/// Handle CS_GET_STORY_BATTLE_SUPPORT_HERO_LIST (13094): report no support
/// heroes. The emulator never recorded this flow; an empty list keeps the
/// deploy screen moving instead of waiting on an unhandled request.
pub async fn handle_get_story_battle_support(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_GET_STORY_BATTLE_SUPPORT_HERO_LIST,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    Ok(vec![build_server_packet(
        13095,
        &SC_GET_STORY_BATTLE_SUPPORT_HERO_LIST {
            dup_type: request.dup_type,
            dup_id: request.dup_id,
            support_list: Vec::new(),
        }
        .encode(),
    )?])
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
