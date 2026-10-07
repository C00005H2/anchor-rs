use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use crate::capture_replay::ReplayCursor;
use crate::messages::pt_hero_formation;

/// A bag entry held in the local player profile.
#[derive(Clone, Debug)]
pub struct BagItem {
    pub id: i32,
    pub tid: i32,
    pub count: i32,
    pub color: i8,
    pub created_time: i32,
    pub expired_time: i32,
}

impl BagItem {
    pub fn from_message(item: &crate::messages::pt_prop_bag) -> Self {
        Self {
            id: item.id,
            tid: item.tid,
            count: item.count,
            color: item.color,
            created_time: item.createdTime,
            expired_time: item.expiredTime,
        }
    }
}

/// A manual skill accepted from the client but not yet represented by the
/// next scripted action batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattlePendingSkill {
    pub hero_id: i32,
    pub skill_id: i32,
}

/// A defender monster in battle with live HP and death state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleMonster {
    pub id: i32,
    pub tid: i32,
    pub current_hp: i64,
    pub max_hp: i64,
}

impl BattleMonster {
    pub fn is_alive(&self) -> bool {
        self.current_hp > 0
    }
}

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
    /// Claimed reward ids per flow, e.g. `("seven_day", 3)`.
    claimed: HashMap<String, HashSet<i64>>,
    /// Local bag contents, keyed by item id, so repeated rewards stay consistent.
    pub bag: HashMap<i32, BagItem>,
    next_bag_item_id: i32,
    /// Last known protocol attribute values (see `SC_PLAYER_UPDATE_ATTR_*`).
    pub attrs: HashMap<i16, String>,
    /// Position inside the scripted battle recording.
    pub battle_script_index: usize,
    /// Whether a battle is currently running for this connection.
    pub battle_active: bool,
    /// Next battle session (battle/session_{n}.json) to try on field enter.
    pub battle_session_index: usize,
    /// Session chosen for the battle currently in progress.
    pub battle_session_chosen: Option<usize>,
    /// Whether the auto-battle push was already served for this battle.
    pub battle_auto_served: bool,
    /// Which recorded action steps of the chosen session were consumed.
    pub battle_step_consumed: Vec<bool>,
    /// Heroes synthesized into the battle entry (not present in the recorded
    /// action batches); they get cloned attack actions so they participate.
    pub battle_added_heroes: Vec<(i32, i32)>,
    /// Recorded attacker hero id -> (deployed id, recorded tid, deployed tid).
    /// Only one-to-one mappings survive; excess captured actors are filtered
    /// so the selected formation is the only attacker roster.
    pub battle_actor_map: HashMap<i32, (i32, i32, i32)>,
    /// The actual attacker lineup used by the current battle: (id, tid, slot).
    pub battle_active_heroes: Vec<(i32, i32, i8)>,
    /// Manually requested skills waiting for a player-side action batch.
    pub battle_pending_skills: VecDeque<BattlePendingSkill>,
    /// Prevent replaying a win result after it has already been delivered.
    pub battle_result_served: bool,
    /// Current round, updated from SC_BATTLE_ACTION_END.
    pub battle_round: i8,
    /// Defender monsters in the current battle, tracking live HP and death state.
    pub battle_monsters: Vec<BattleMonster>,
    /// Round during which each added hero last performed a basic turn action.
    pub battle_added_heroes_acted_round: HashMap<i32, i8>,
    /// Whether this connection has reported its formation at least once.
    pub formation_received: bool,
    /// The team selected by CS_SET_READY, when one has been selected.
    pub ready_team_id: Option<i16>,
    /// Position inside the recorded hero-recruit prepare sequence.
    pub recruit_prepare_index: usize,
    /// Position inside the recorded hero-recruit save-list sequence.
    pub recruit_save_index: usize,
    /// Latest battle sync word observed in scripted battle responses.
    pub battle_sync_word: i32,
    /// Sync word whose next video-end request was already answered when auto
    /// mode proactively pushed the next recorded action.
    pub battle_auto_resume_sync_word: Option<i32>,
    /// Stage ids claimed via CS_MAIN_STORY_STAGE_AWARD this connection.
    pub story_award_claimed: Vec<i32>,
    /// Indices of story-pass recordings already consumed this connection.
    pub story_pass_taken: Vec<usize>,
    /// Current field / stage id being played in battle (e.g. "1001").
    pub battle_current_field_id: Option<String>,
    /// Passed main story stage ids.
    pub story_pass_stage_list: Vec<i32>,
    /// Current available main story stage id(s) — strictly length <= 1.
    pub story_now_stage_list: Vec<i32>,
    /// Unlocked chapter splash picture ids (chapters 1..=10).
    pub story_play_chapter_pic_list: Vec<i16>,
    /// Evolution level reached per hero instance this connection.
    pub hero_evolution: HashMap<i32, i16>,
    /// Hero formation last reported by `CS_CHANGE_HERO`.
    pub formation: Vec<pt_hero_formation>,
    /// Purchase counts per goods id.
    purchases: HashMap<i32, i16>,
    /// Panel request counters (e.g. direct gift panel).
    request_counts: HashMap<String, i16>,
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
            claimed: HashMap::new(),
            bag: HashMap::new(),
            next_bag_item_id: 1_000,
            attrs: HashMap::new(),
            battle_script_index: 0,
            battle_active: false,
            battle_session_index: 0,
            battle_session_chosen: None,
            battle_auto_served: false,
            battle_step_consumed: Vec::new(),
            battle_added_heroes: Vec::new(),
            battle_actor_map: HashMap::new(),
            battle_active_heroes: Vec::new(),
            battle_pending_skills: VecDeque::new(),
            battle_result_served: false,
            battle_round: 0,
            battle_monsters: Vec::new(),
            battle_added_heroes_acted_round: HashMap::new(),
            formation_received: false,
            ready_team_id: None,
            recruit_prepare_index: 0,
            recruit_save_index: 0,
            battle_sync_word: 0,
            battle_auto_resume_sync_word: None,
            story_award_claimed: Vec::new(),
            story_pass_taken: Vec::new(),
            battle_current_field_id: None,
            story_pass_stage_list: vec![1004, 1003, 1002, 1001],
            story_now_stage_list: vec![1005],
            story_play_chapter_pic_list: (1..=10).collect(),
            hero_evolution: HashMap::new(),
            formation: Vec::new(),
            purchases: HashMap::new(),
            request_counts: HashMap::new(),
        }
    }

    /// Clear per-battle replay state without changing the player's formation or
    /// the round-robin index used to choose the next recorded session.
    pub fn clear_battle_runtime(&mut self) {
        self.battle_active = false;
        self.battle_session_chosen = None;
        self.battle_step_consumed.clear();
        self.battle_auto_served = false;
        self.battle_script_index = 0;
        self.battle_added_heroes.clear();
        self.battle_actor_map.clear();
        self.battle_active_heroes.clear();
        self.battle_pending_skills.clear();
        self.battle_result_served = true;
        self.battle_round = 0;
        self.battle_monsters.clear();
        self.battle_added_heroes_acted_round.clear();
        self.battle_sync_word = 0;
        self.battle_auto_resume_sync_word = None;
        self.battle_current_field_id = None;
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

    /// Mark a reward as claimed, returning `false` when it was claimed before.
    pub fn claim_once(&mut self, flow: &str, id: i64) -> bool {
        self.claimed
            .entry(flow.to_owned())
            .or_default()
            .insert(id)
    }

    pub fn is_claimed(&self, flow: &str, id: i64) -> bool {
        self.claimed
            .get(flow)
            .is_some_and(|ids| ids.contains(&id))
    }

    /// Ids of a flow that were already claimed, ascending.
    pub fn claimed_ids(&self, flow: &str) -> Vec<i64> {
        let mut ids: Vec<i64> = self
            .claimed
            .get(flow)
            .map(|ids| ids.iter().copied().collect())
            .unwrap_or_default();
        ids.sort_unstable();
        ids
    }

    /// Claimed ids narrowed to the protocol's 16-bit id fields.
    pub fn claimed_ids_i16(&self, flow: &str) -> Vec<i16> {
        self.claimed_ids(flow)
            .into_iter()
            .map(|id| id as i16)
            .collect()
    }

    pub fn claimed_ids_i32(&self, flow: &str) -> Vec<i32> {
        self.claimed_ids(flow)
            .into_iter()
            .map(|id| id as i32)
            .collect()
    }

    /// Allocate the next local bag item id.
    pub fn allocate_bag_item(&mut self) -> i32 {
        let id = self.next_bag_item_id;
        self.next_bag_item_id = self.next_bag_item_id.saturating_add(1);
        id
    }

    /// Add `count` items of `tid` to the local bag.  Existing stacks grow;
    /// a new stack keeps the granted color.  Returns `(id, total, color)`.
    pub fn grant_bag_item(&mut self, tid: i32, count: i32, color: i8) -> (i32, i32, i8) {
        let existing_id = self
            .bag
            .values()
            .find(|item| item.tid == tid)
            .map(|item| item.id);

        match existing_id {
            Some(id) => {
                let stored = self
                    .bag
                    .get_mut(&id)
                    .expect("item id looked up from the bag map");
                stored.count = stored.count.saturating_add(count);
                (stored.id, stored.count, stored.color)
            }
            None => {
                let id = self.allocate_bag_item();
                self.bag.insert(
                    id,
                    BagItem {
                        id,
                        tid,
                        count,
                        color,
                        created_time: 0,
                        expired_time: 0,
                    },
                );
                (id, count, color)
            }
        }
    }

    /// Store granted items so the local profile reflects the rewards.
    pub fn store_items<'a>(&mut self, items: impl IntoIterator<Item = &'a BagItem>) {
        for item in items {
            self.bag.insert(item.id, item.clone());
        }
    }

    /// Remember the newest value of a protocol attribute.
    pub fn record_attr(&mut self, key: i16, value: String) {
        self.attrs.insert(key, value);
    }

    pub fn attr_value(&self, key: i16) -> Option<&str> {
        self.attrs.get(&key).map(String::as_str)
    }

    /// Record a purchase; returns the new total for the goods.
    pub fn record_purchase(&mut self, goods_id: i32, num: i16) -> i16 {
        let total = self.purchases.entry(goods_id).or_insert(0);
        *total = total.saturating_add(num);
        *total
    }

    pub fn purchase_count(&self, goods_id: i32) -> i16 {
        self.purchases.get(&goods_id).copied().unwrap_or(0)
    }

    /// Count how often a panel was requested in this session.
    pub fn bump_request_count(&mut self, panel: &str) -> i16 {
        let count = self.request_counts.entry(panel.to_owned()).or_insert(0);
        *count = count.saturating_add(1);
        *count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_are_recorded_once_per_flow() {
        let mut connection = ConnectionContext::new("session".to_owned());
        assert!(connection.claim_once("seven_day", 1));
        assert!(!connection.claim_once("seven_day", 1));
        assert!(connection.is_claimed("seven_day", 1));
        assert!(!connection.is_claimed("open_server_sign", 1));
        assert!(connection.claim_once("open_server_sign", 1));

        assert_eq!(connection.claimed_ids("seven_day"), vec![1]);
        assert_eq!(connection.claimed_ids_i16("seven_day"), vec![1i16]);
    }

    #[test]
    fn bag_items_merge_by_template_id() {
        let mut connection = ConnectionContext::new("session".to_owned());
        let (first_id, first_count, _) = connection.grant_bag_item(2013, 6, 4);
        let (second_id, second_count, color) = connection.grant_bag_item(2013, 20, 0);
        assert_eq!(first_id, second_id);
        assert_eq!(first_count, 6);
        assert_eq!(second_count, 26);
        assert_eq!(color, 4);

        let (other_id, _, _) = connection.grant_bag_item(2201, 10, 3);
        assert_ne!(other_id, first_id);
    }

    #[test]
    fn bag_item_ids_are_unique_and_increasing() {
        let mut connection = ConnectionContext::new("session".to_owned());
        let first = connection.allocate_bag_item();
        let second = connection.allocate_bag_item();
        assert!(second > first);
    }

    #[test]
    fn initial_story_state_has_valid_invariants() {
        let connection = ConnectionContext::new("session".to_owned());
        assert_eq!(connection.story_now_stage_list.len(), 1);
        assert_eq!(connection.story_now_stage_list, vec![1005]);
        assert_eq!(connection.story_pass_stage_list, vec![1004, 1003, 1002, 1001]);
        assert!(!connection.story_pass_stage_list.contains(&1005));
        assert_eq!(connection.story_play_chapter_pic_list.len(), 10);
    }
}