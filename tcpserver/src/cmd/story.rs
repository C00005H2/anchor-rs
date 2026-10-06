//! Main-story flows: stage completion and the "skip duplicate story" pass.

use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use crate::{
    cmd::battle::absorb_attr_updates,
    messages::{CS_DUP_ONLY_STORY_PASS, CS_STORY_OVER},
    sequence::TemplateFile,
    state::ConnectionContext,
};

const STORY_PASS_DATA: &str = "story/dup_only_story_pass.json";

/// Handle CS_STORY_OVER (12054): the real server sent no reply.
pub async fn handle_story_over(
    ctx: Arc<Mutex<ConnectionContext>>,
    _request: CS_STORY_OVER,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    ctx.lock().await.update_heartbeat();
    info!("Story dialogue finished");
    Ok(Vec::new())
}

/// Handle CS_DUP_ONLY_STORY_PASS (18012): replay the recorded skip-pass flow
/// (story/manual updates, unread notices, bag and attribute rewards).
pub async fn handle_dup_only_story_pass(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_DUP_ONLY_STORY_PASS,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let script = TemplateFile::load(STORY_PASS_DATA)?;
    let Some(group) = script.first_group().cloned() else {
        return Ok(Vec::new());
    };

    let cursor = ctx.lock().await.replay_cursor.clone();
    info!(
        battle_type = request.battle_type,
        field_id = request.field_id,
        "Story stage skipped (scripted replay)"
    );
    let packets = group.encode(&cursor)?;
    {
        let mut connection = ctx.lock().await;
        absorb_attr_updates(&mut connection, &group);
    }
    Ok(packets)
}
