use std::collections::HashMap;
use std::time::Instant;

use crate::capture_replay::ReplayCursor;

pub struct ConnectionContext {
    pub player_id: Option<i64>,
    pub session_id: String,
    pub logged_in: bool,
    pub last_heartbeat: Instant,
    /// Per-client cursor and anonymized IDs used by optional capture replay.
    pub replay_cursor: ReplayCursor,
    /// Track dialogue progression for each NPC
    /// Key: "dialogue_{npc_id}", Value: current dialogue part
    pub dialogue_state: HashMap<String, i32>,
}

impl ConnectionContext {
    pub fn new(session_id: String) -> Self {
        Self {
            player_id: None,
            session_id,
            logged_in: false,
            last_heartbeat: Instant::now(),
            replay_cursor: ReplayCursor::default(),
            dialogue_state: HashMap::new(),
        }
    }

    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = Instant::now();
    }

    pub fn is_authenticated(&self) -> bool {
        self.logged_in && self.player_id.is_some()
    }

    /// Get current dialogue part for an NPC
    pub fn get_dialogue_part(&self, npc_id: i32) -> Option<i32> {
        let key = format!("dialogue_{}", npc_id);
        self.dialogue_state.get(&key).copied()
    }

    /// Set dialogue part for an NPC
    pub fn set_dialogue_part(&mut self, npc_id: i32, part: i32) {
        let key = format!("dialogue_{}", npc_id);
        self.dialogue_state.insert(key, part);
    }

    /// Advance dialogue to next part
    pub fn advance_dialogue(&mut self, npc_id: i32) -> i32 {
        let current = self.get_dialogue_part(npc_id).unwrap_or(0);
        let next_part = current.saturating_add(1);
        self.set_dialogue_part(npc_id, next_part);
        next_part
    }

    /// Reset dialogue for an NPC (start over)
    pub fn reset_dialogue(&mut self, npc_id: i32) {
        let key = format!("dialogue_{}", npc_id);
        self.dialogue_state.remove(&key);
    }

    /// Check if dialogue has been started with an NPC
    pub fn has_talked_to(&self, npc_id: i32) -> bool {
        self.get_dialogue_part(npc_id).is_some()
    }
}