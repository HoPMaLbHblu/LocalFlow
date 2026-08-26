//! Draft state: merging captures, corrections, saving. OWNER: vision agent.
//!
//! Screenshots only ever add or improve information: an empty reading never erases a hero,
//! a reading replaces another only when it is more confident, and the player's corrections
//! (`Manual`) and Game State Integration (`Gsi`) are never overwritten by a screenshot.

use std::{collections::HashSet, path::PathBuf};

use super::{data_dir, now, write_atomic, DraftState, PickSource, Recognition, Side, Slot, Team, UNCERTAIN};

fn draft_path() -> PathBuf {
    data_dir().join("draft.json")
}

fn locked(slot: &Slot) -> bool {
    matches!(slot.source, Some(PickSource::Manual | PickSource::Gsi))
}

impl DraftState {
    /// The saved draft, or a fresh one. A corrupt file is kept as `draft.corrupt-<time>.json`.
    pub fn load() -> DraftState {
        let path = draft_path();
        let Ok(text) = std::fs::read_to_string(&path) else { return DraftState::default() };
        match serde_json::from_str(&text) {
            Ok(state) => state,
            Err(e) => {
                let keep = path.with_file_name(format!("draft.corrupt-{}.json", now()));
                let _ = std::fs::rename(&path, &keep);
                tracing::warn!("draft.json was unreadable ({e}); kept it as {} and started fresh", keep.display());
                DraftState::default()
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(&draft_path(), text.as_bytes())
    }

    /// True while the player's team is unknown and Radiant is assumed to be the allies.
    pub fn team_assumed(&self) -> bool {
        self.player_team.is_none()
    }

    /// The team shown as "allies" (Radiant when unknown).
    pub fn ally_team(&self) -> Team {
        self.player_team.unwrap_or(Team::Radiant)
    }

    /// Which side a screen team is on for this player.
    pub fn side_of(&self, team: Team) -> Side {
        if team == self.ally_team() {
            Side::Allies
        } else {
            Side::Enemies
        }
    }

    /// Sets the player's team. If that changes which screen half is "allies", the sides swap.
    pub fn set_player_team(&mut self, team: Option<Team>) {
        let before = self.ally_team();
        self.player_team = team;
        if self.ally_team() != before {
            std::mem::swap(&mut self.allies, &mut self.enemies);
        }
        self.updated_at = now();
    }

    pub fn slots(&self, side: Side) -> &[Slot; 5] {
        match side {
            Side::Allies => &self.allies,
            Side::Enemies => &self.enemies,
        }
    }

    pub fn slots_mut(&mut self, side: Side) -> &mut [Slot; 5] {
        match side {
            Side::Allies => &mut self.allies,
            Side::Enemies => &mut self.enemies,
        }
    }

    /// Where a hero currently is.
    pub fn find(&self, hero_id: u32) -> Option<(Side, u8)> {
        for side in [Side::Allies, Side::Enemies] {
            if let Some(i) = self.slots(side).iter().position(|s| s.hero_id == Some(hero_id)) {
                return Some((side, i as u8));
            }
        }
        None
    }

    /// Adds one screenshot's reading (see the module docs for the rules).
    pub fn merge(&mut self, recognition: &Recognition) {
        let mut picks: Vec<_> = recognition.picks.iter().filter(|p| p.slot < 5).collect();
        picks.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        for pick in picks {
            let side = self.side_of(pick.team);
            let index = pick.slot as usize;
            let target = &self.slots(side)[index];
            if locked(target) {
                continue;
            }
            if target.hero_id == Some(pick.hero_id) {
                let target = &mut self.slots_mut(side)[index];
                if pick.confidence > target.confidence {
                    target.confidence = pick.confidence;
                    target.alternatives = pick.alternatives.clone();
                }
                continue;
            }
            if target.hero_id.is_some() && pick.confidence <= target.confidence {
                continue;
            }
            // The same hero elsewhere: keep the more confident (and any locked) one.
            if let Some((other_side, other)) = self.find(pick.hero_id) {
                let slot = &self.slots(other_side)[other as usize];
                if locked(slot) || slot.confidence >= pick.confidence {
                    continue;
                }
                self.slots_mut(other_side)[other as usize] = Slot::default();
            }
            self.slots_mut(side)[index] = Slot {
                hero_id: Some(pick.hero_id),
                confidence: pick.confidence,
                source: Some(PickSource::Screenshot),
                alternatives: pick.alternatives.clone(),
            };
        }
        self.captures += 1;
        self.updated_at = now();
    }

    /// The player's correction. `Some(hero)` fixes the slot (never overwritten by screenshots)
    /// and removes that hero from any other slot; `None` empties the slot so a later
    /// screenshot may fill it again.
    pub fn correct(&mut self, side: Side, slot: u8, hero_id: Option<u32>) {
        if slot >= 5 {
            return;
        }
        match hero_id {
            Some(id) => {
                for s in self.allies.iter_mut().chain(self.enemies.iter_mut()) {
                    if s.hero_id == Some(id) {
                        *s = Slot::default();
                    }
                }
                self.slots_mut(side)[slot as usize] =
                    Slot { hero_id: Some(id), confidence: 1.0, source: Some(PickSource::Manual), alternatives: Vec::new() };
            }
            None => self.slots_mut(side)[slot as usize] = Slot::default(),
        }
        self.updated_at = now();
    }

    /// The player's own hero (from GSI or chosen). Also puts it in an allied slot.
    pub fn set_player_hero(&mut self, hero_id: Option<u32>, source: PickSource) {
        self.player_hero = hero_id;
        self.updated_at = now();
        let Some(id) = hero_id else { return };
        let source = if source == PickSource::Screenshot { PickSource::Manual } else { source };
        let fixed = Slot { hero_id: Some(id), confidence: 1.0, source: Some(source), alternatives: Vec::new() };
        match self.find(id) {
            Some((Side::Allies, i)) => {
                let s = &mut self.allies[i as usize];
                if !locked(s) || source == PickSource::Gsi {
                    *s = fixed;
                }
                return;
            }
            Some((Side::Enemies, i)) => self.enemies[i as usize] = Slot::default(),
            None => {}
        }
        // An empty allied slot, else the least confident screenshot guess.
        let index = self.allies.iter().position(|s| s.hero_id.is_none()).or_else(|| {
            self.allies
                .iter()
                .enumerate()
                .filter(|(_, s)| !locked(s))
                .min_by(|a, b| a.1.confidence.total_cmp(&b.1.confidence))
                .map(|(i, _)| i)
        });
        if let Some(i) = index {
            self.allies[i] = fixed;
        }
    }

    pub fn reset(&mut self) {
        *self = DraftState { updated_at: now(), ..DraftState::default() };
    }

    /// Slots with a hero below `UNCERTAIN` confidence, allies first.
    pub fn uncertain(&self) -> Vec<(Side, u8)> {
        let mut out = Vec::new();
        for side in [Side::Allies, Side::Enemies] {
            for (i, s) in self.slots(side).iter().enumerate() {
                if s.hero_id.is_some() && s.confidence < UNCERTAIN {
                    out.push((side, i as u8));
                }
            }
        }
        out
    }

    pub fn picked(&self) -> HashSet<u32> {
        self.allies.iter().chain(self.enemies.iter()).filter_map(|s| s.hero_id).collect()
    }

    /// All ten heroes are known.
    pub fn is_complete(&self) -> bool {
        self.allies.iter().chain(self.enemies.iter()).all(|s| s.hero_id.is_some())
    }
}
