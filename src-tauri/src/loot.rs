//! Local loot tracking: observation opportunities (kills) + item drops by zone/mob.
//!
//! **Denominator (`kills`)** = observation opportunities for drop rates:
//! 1. Personal/pet kill credit (`You have slain`, `slain by You`, configured pet), and
//! 2. First loot interaction on a corpse that was not already counted (group/other kills you loot).
//! Multiple item lines from the same corpse are deduped via a short per-mob session window.

use crate::parser::LogEvent;
use crate::pets::is_my_pet;
use crate::spell_db::{config_path, AppConfig};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// Attach coin lines to a recent opportunity.
const COIN_WINDOW: Duration = Duration::from_secs(45);
/// Attach loot to a prior personal/pet kill for the same mob.
const KILL_LOOT_WINDOW: Duration = Duration::from_secs(90);
/// Subsequent loot lines for the same mob within this gap count as the same corpse.
const SAME_CORPSE_BURST: Duration = Duration::from_secs(12);
const RECENT_CAP: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ItemDropStats {
    /// Times this item appeared on a corpse (one loot line = one appearance).
    pub times: u64,
    /// Sum of stack quantities across appearances.
    pub quantity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MobLootStats {
    pub display_name: String,
    /// Observation opportunities (personal/pet kills + first loot on uncounted corpses).
    pub kills: u64,
    pub items: HashMap<String, ItemDropStats>,
    pub coin_copper_total: u64,
    pub coin_samples: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LootFile {
    pub version: u32,
    /// zone display name -> mob_key -> stats
    pub zones: HashMap<String, HashMap<String, MobLootStats>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LootMobRow {
    pub zone: String,
    pub mob: String,
    pub mob_key: String,
    pub kills: u64,
    pub unique_items: usize,
    pub coin_avg_copper: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LootItemRow {
    pub zone: String,
    pub mob: String,
    pub mob_key: String,
    pub item: String,
    pub times: u64,
    pub quantity: u64,
    pub kills: u64,
    /// `times / kills` when kills > 0, else null.
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LootSnapshot {
    pub total_kills: u64,
    pub total_item_appearances: u64,
    pub mob_count: usize,
    pub zone: Option<String>,
    pub mobs: Vec<LootMobRow>,
    pub items: Vec<LootItemRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootSyncItem {
    pub item: String,
    pub times: u64,
    pub quantity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootSyncMob {
    pub zone: String,
    pub mob: String,
    pub kills: u64,
    #[serde(rename = "coinCopperTotal")]
    pub coin_copper_total: u64,
    #[serde(rename = "coinSamples")]
    pub coin_samples: u64,
    pub items: Vec<LootSyncItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootSyncPayload {
    #[serde(rename = "contributorId")]
    pub contributor_id: String,
    pub app: String,
    pub version: u32,
    pub mobs: Vec<LootSyncMob>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LootSyncResult {
    pub ok: bool,
    pub message: String,
    pub kills_added: Option<u64>,
    pub drops_added: Option<u64>,
}

/// In-memory corpse/kill opportunity used for dedupe + coin correlation.
#[derive(Debug, Clone)]
struct RecentOpportunity {
    mob_key: String,
    display: String,
    /// When the opportunity opened (kill or first loot).
    opened_at: chrono::DateTime<Utc>,
    /// Last kill/loot/coin touch on this opportunity.
    last_activity: chrono::DateTime<Utc>,
    coin_claimed: bool,
    /// Opened by personal/pet Death (already +1 kills).
    from_kill: bool,
    /// At least one item loot line attached.
    loot_seen: bool,
}

pub struct LootEngine {
    data: LootFile,
    zone: Option<String>,
    recent: VecDeque<RecentOpportunity>,
    dirty: bool,
}

impl LootEngine {
    pub fn new() -> Self {
        Self {
            data: load_loot_file(),
            zone: None,
            recent: VecDeque::new(),
            dirty: false,
        }
    }

    pub fn set_zone(&mut self, zone: &str) {
        let z = zone.trim();
        if z.is_empty() {
            self.zone = None;
        } else {
            self.zone = Some(z.to_string());
        }
    }

    pub fn current_zone(&self) -> Option<&str> {
        self.zone.as_deref()
    }

    pub fn handle(&mut self, event: LogEvent, config: &AppConfig) -> bool {
        if !config.loot_tracking {
            return false;
        }
        match event {
            LogEvent::ZoneChange { zone } => {
                self.set_zone(&zone);
                false
            }
            // Personal/pet kill credit: always +1 opportunity.
            LogEvent::Death {
                target,
                by_you,
                killer,
            } if by_you
                || killer
                    .as_deref()
                    .is_some_and(|k| is_my_pet(k, &config.my_pet_name)) =>
            {
                self.record_kill(&target);
                true
            }
            LogEvent::LootItem {
                item,
                quantity,
                mob,
                ..
            } => {
                self.record_item(&mob, &item, quantity);
                true
            }
            LogEvent::CorpseCoin { copper } => self.record_coin(copper),
            _ => false,
        }
    }

    fn zone_key(&self) -> String {
        self.zone.clone().unwrap_or_else(|| "Unknown".to_string())
    }

    fn age(now: chrono::DateTime<Utc>, then: chrono::DateTime<Utc>) -> Duration {
        now.signed_duration_since(then)
            .to_std()
            .unwrap_or(Duration::MAX)
    }

    fn push_recent(&mut self, opp: RecentOpportunity) {
        self.recent.push_front(opp);
        while self.recent.len() > RECENT_CAP {
            self.recent.pop_back();
        }
    }

    fn bump_kills(&mut self, key: &str, display: &str) {
        let zone = self.zone_key();
        let mob = self
            .data
            .zones
            .entry(zone)
            .or_default()
            .entry(key.to_string())
            .or_insert_with(|| MobLootStats {
                display_name: display.to_string(),
                ..Default::default()
            });
        if mob.display_name.is_empty() {
            mob.display_name = display.to_string();
        }
        mob.kills = mob.kills.saturating_add(1);
        self.dirty = true;
    }

    fn record_kill(&mut self, target: &str) {
        let (key, display) = normalize_mob(target);
        self.bump_kills(&key, &display);
        let now = Utc::now();
        self.push_recent(RecentOpportunity {
            mob_key: key,
            display,
            opened_at: now,
            last_activity: now,
            coin_claimed: false,
            from_kill: true,
            loot_seen: false,
        });
    }

    /// Record an item drop; +1 kill only when this opens a new observation opportunity.
    fn record_item(&mut self, mob_raw: &str, item: &str, quantity: u32) {
        let (key, display) = normalize_mob(mob_raw);
        let item_name = item.trim();
        if item_name.is_empty() {
            return;
        }
        let qty = quantity.max(1) as u64;
        let now = Utc::now();

        // Dedupe / attach to an existing opportunity for this mob.
        let attach_idx = self.find_loot_attach_index(&key, now);
        if let Some(idx) = attach_idx {
            self.recent[idx].loot_seen = true;
            self.recent[idx].last_activity = now;
            // Move to front so coin prefers this corpse.
            let opp = self.recent.remove(idx).expect("index checked");
            self.recent.push_front(opp);
        } else {
            // First loot on an uncounted corpse (group/other kill you looted).
            self.bump_kills(&key, &display);
            self.push_recent(RecentOpportunity {
                mob_key: key.clone(),
                display: display.clone(),
                opened_at: now,
                last_activity: now,
                coin_claimed: false,
                from_kill: false,
                loot_seen: true,
            });
        }

        let zone = self.zone_key();
        let mob = self
            .data
            .zones
            .entry(zone)
            .or_default()
            .entry(key)
            .or_insert_with(|| MobLootStats {
                display_name: display.clone(),
                ..Default::default()
            });
        if mob.display_name.is_empty() {
            mob.display_name = display;
        }
        let drop = mob.items.entry(item_name.to_string()).or_default();
        drop.times = drop.times.saturating_add(1);
        drop.quantity = drop.quantity.saturating_add(qty);
        self.dirty = true;
    }

    /// Prefer same-corpse burst, else an unlooted personal/pet kill within the loot window.
    fn find_loot_attach_index(&self, mob_key: &str, now: chrono::DateTime<Utc>) -> Option<usize> {
        // 1) Active loot session for this mob (multi-item same corpse).
        if let Some(idx) = self.recent.iter().position(|o| {
            o.mob_key == mob_key
                && o.loot_seen
                && Self::age(now, o.last_activity) <= SAME_CORPSE_BURST
        }) {
            return Some(idx);
        }
        // 2) Personal/pet kill waiting for first loot.
        self.recent.iter().position(|o| {
            o.mob_key == mob_key
                && o.from_kill
                && !o.loot_seen
                && Self::age(now, o.opened_at) <= KILL_LOOT_WINDOW
        })
    }

    fn record_coin(&mut self, copper: u64) -> bool {
        let now = Utc::now();
        let idx = self
            .recent
            .iter()
            .position(|k| !k.coin_claimed && Self::age(now, k.last_activity) <= COIN_WINDOW);
        let Some(idx) = idx else {
            return false;
        };
        let kill = self.recent[idx].clone();
        self.recent[idx].coin_claimed = true;
        self.recent[idx].last_activity = now;

        let zone = self.zone_key();
        let mob = self
            .data
            .zones
            .entry(zone)
            .or_default()
            .entry(kill.mob_key)
            .or_insert_with(|| MobLootStats {
                display_name: kill.display,
                ..Default::default()
            });
        mob.coin_copper_total = mob.coin_copper_total.saturating_add(copper);
        mob.coin_samples = mob.coin_samples.saturating_add(1);
        self.dirty = true;
        true
    }

    pub fn flush_if_dirty(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        save_loot_file(&self.data)?;
        self.dirty = false;
        Ok(())
    }

    pub fn clear_all(&mut self) -> Result<(), String> {
        self.data = LootFile {
            version: 1,
            zones: HashMap::new(),
        };
        self.recent.clear();
        self.dirty = false;
        save_loot_file(&self.data)
    }

    /// Payload for Norrath Roster `POST /api/loot/ingest`.
    pub fn export_for_sync(&self, contributor_id: &str) -> LootSyncPayload {
        let mut mobs = Vec::new();
        for (zone, zone_mobs) in &self.data.zones {
            for (_key, stats) in zone_mobs {
                let items = stats
                    .items
                    .iter()
                    .map(|(item, drop)| LootSyncItem {
                        item: item.clone(),
                        times: drop.times,
                        quantity: drop.quantity,
                    })
                    .collect();
                mobs.push(LootSyncMob {
                    zone: zone.clone(),
                    mob: stats.display_name.clone(),
                    kills: stats.kills,
                    coin_copper_total: stats.coin_copper_total,
                    coin_samples: stats.coin_samples,
                    items,
                });
            }
        }
        LootSyncPayload {
            contributor_id: contributor_id.to_string(),
            app: "berryworks".into(),
            version: self.data.version.max(1),
            mobs,
        }
    }

    pub fn snapshot(&self, query: &str) -> LootSnapshot {
        let q = query.trim().to_lowercase();
        let mut mobs = Vec::new();
        let mut items = Vec::new();
        let mut total_kills = 0u64;
        let mut total_item_appearances = 0u64;
        let mut mob_count = 0usize;

        for (zone, zone_mobs) in &self.data.zones {
            for (mob_key, stats) in zone_mobs {
                total_kills = total_kills.saturating_add(stats.kills);
                mob_count += 1;
                let mob_match = q.is_empty()
                    || stats.display_name.to_lowercase().contains(&q)
                    || mob_key.contains(&q)
                    || zone.to_lowercase().contains(&q);

                let coin_avg = if stats.coin_samples > 0 {
                    stats.coin_copper_total / stats.coin_samples
                } else {
                    0
                };

                if mob_match {
                    mobs.push(LootMobRow {
                        zone: zone.clone(),
                        mob: stats.display_name.clone(),
                        mob_key: mob_key.clone(),
                        kills: stats.kills,
                        unique_items: stats.items.len(),
                        coin_avg_copper: coin_avg,
                    });
                }

                for (item, drop) in &stats.items {
                    total_item_appearances = total_item_appearances.saturating_add(drop.times);
                    let item_match = q.is_empty()
                        || item.to_lowercase().contains(&q)
                        || stats.display_name.to_lowercase().contains(&q)
                        || zone.to_lowercase().contains(&q);
                    if !item_match {
                        continue;
                    }
                    let rate = if stats.kills > 0 {
                        Some(drop.times as f64 / stats.kills as f64)
                    } else {
                        None
                    };
                    items.push(LootItemRow {
                        zone: zone.clone(),
                        mob: stats.display_name.clone(),
                        mob_key: mob_key.clone(),
                        item: item.clone(),
                        times: drop.times,
                        quantity: drop.quantity,
                        kills: stats.kills,
                        rate,
                    });
                }
            }
        }

        mobs.sort_by(|a, b| {
            b.kills
                .cmp(&a.kills)
                .then_with(|| a.mob.to_lowercase().cmp(&b.mob.to_lowercase()))
                .then_with(|| a.zone.cmp(&b.zone))
        });
        items.sort_by(|a, b| {
            b.times
                .cmp(&a.times)
                .then_with(|| a.item.to_lowercase().cmp(&b.item.to_lowercase()))
                .then_with(|| a.mob.cmp(&b.mob))
        });

        LootSnapshot {
            total_kills,
            total_item_appearances,
            mob_count,
            zone: self.zone.clone(),
            mobs,
            items,
        }
    }
}

/// Normalize mob names for aggregation: lowercase, strip leading a/an.
pub fn normalize_mob(raw: &str) -> (String, String) {
    let display = raw.trim().to_string();
    let mut key = display.to_lowercase();
    if let Some(rest) = key.strip_prefix("an ") {
        key = rest.trim().to_string();
    } else if let Some(rest) = key.strip_prefix("a ") {
        key = rest.trim().to_string();
    }
    (key, display)
}

pub fn loot_path() -> PathBuf {
    config_path()
        .parent()
        .map(|p| p.join("loot.json"))
        .unwrap_or_else(|| PathBuf::from("loot.json"))
}

fn load_loot_file() -> LootFile {
    let path = loot_path();
    if !path.exists() {
        return LootFile {
            version: 1,
            zones: HashMap::new(),
        };
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| LootFile {
            version: 1,
            zones: HashMap::new(),
        })
}

fn save_loot_file(data: &LootFile) -> Result<(), String> {
    let path = loot_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_line;

    fn eng_in(zone: &str) -> LootEngine {
        LootEngine {
            data: LootFile {
                version: 1,
                zones: HashMap::new(),
            },
            zone: Some(zone.into()),
            recent: VecDeque::new(),
            dirty: false,
        }
    }

    fn cfg_tracking() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.loot_tracking = true;
        cfg
    }

    #[test]
    fn normalize_strips_articles() {
        let (k, d) = normalize_mob("an ire ghast");
        assert_eq!(k, "ire ghast");
        assert_eq!(d, "an ire ghast");
        let (k2, _) = normalize_mob("Unbound Flame");
        assert_eq!(k2, "unbound flame");
    }

    #[test]
    fn tracks_kill_and_loot_rate() {
        let mut eng = eng_in("Plane of Hate");
        let cfg = cfg_tracking();

        eng.handle(
            parse_line("[Tue Aug 11 23:15:37 2026] You have slain an ogre guard!"),
            &cfg,
        );
        eng.handle(
            parse_line(
                "[Tue Aug 11 23:15:39 2026] You looted a Bronze Dagger +4 from an ogre guard's corpse and sold it for 2 gold.",
            ),
            &cfg,
        );
        eng.handle(
            parse_line("[Tue Aug 11 23:15:40 2026] You have slain an ogre guard!"),
            &cfg,
        );

        let snap = eng.snapshot("ogre");
        assert_eq!(snap.mobs.len(), 1);
        assert_eq!(snap.mobs[0].kills, 2);
        let dagger = snap
            .items
            .iter()
            .find(|i| i.item.contains("Bronze Dagger"))
            .expect("dagger");
        assert_eq!(dagger.times, 1);
        assert!((dagger.rate.unwrap() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn kill_then_loot_does_not_double_count() {
        let mut eng = eng_in("Plane of Hate");
        let cfg = cfg_tracking();

        eng.handle(
            parse_line("[Tue Aug 11 23:15:37 2026] You have slain an ogre guard!"),
            &cfg,
        );
        eng.handle(
            parse_line(
                "[Tue Aug 11 23:15:39 2026] You looted a Bronze Dagger +4 from an ogre guard's corpse and sold it for 2 gold.",
            ),
            &cfg,
        );
        eng.handle(
            parse_line(
                "[Tue Aug 11 23:15:39 2026] --You have looted a Golden Earring +4 from an ogre guard's corpse.--",
            ),
            &cfg,
        );

        let snap = eng.snapshot("ogre");
        assert_eq!(
            snap.mobs[0].kills, 1,
            "kill + multi-item loot = one opportunity"
        );
        assert_eq!(snap.total_item_appearances, 2);
    }

    #[test]
    fn loot_only_counts_one_kill_on_first_item() {
        let mut eng = eng_in("Plane of Hate");
        let cfg = cfg_tracking();

        eng.handle(
            parse_line(
                "[Wed Aug 12 00:00:20 2026] You looted 2 Crystallized Sulfur from an ire ghast's corpse and sold it for 2 gold.",
            ),
            &cfg,
        );
        let snap = eng.snapshot("ire");
        assert_eq!(snap.mobs[0].kills, 1);
        assert_eq!(snap.items[0].times, 1);
        assert!((snap.items[0].rate.unwrap() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn second_item_same_corpse_does_not_increment_kill() {
        let mut eng = eng_in("Plane of Hate");
        let cfg = cfg_tracking();

        eng.handle(
            parse_line(
                "[Wed Aug 12 00:00:20 2026] You looted 2 Crystallized Sulfur from an ire ghast's corpse and sold it for 2 gold.",
            ),
            &cfg,
        );
        eng.handle(
            parse_line(
                "[Wed Aug 12 00:00:21 2026] You looted a Valorium Vambraces from an ire ghast's corpse to create a Valorium Vambraces +1",
            ),
            &cfg,
        );

        let snap = eng.snapshot("ire");
        assert_eq!(snap.mobs[0].kills, 1);
        assert_eq!(snap.total_item_appearances, 2);
        let sulfur = snap
            .items
            .iter()
            .find(|i| i.item.contains("Sulfur"))
            .unwrap();
        assert!((sulfur.rate.unwrap() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn counts_pet_kill_when_my_pet_name_set() {
        let mut eng = eng_in("Lower Guk");
        let mut cfg = cfg_tracking();
        cfg.my_pet_name = "Gastik".into();

        assert!(!eng.handle(
            parse_line("[Thu Aug 06 21:51:20 2026] A frenzied ghoul has been slain by Vebn!",),
            &cfg,
        ));
        assert!(eng.handle(
            parse_line("[Thu Aug 06 21:51:21 2026] A frenzied ghoul has been slain by Gastik!",),
            &cfg,
        ));
        assert_eq!(eng.snapshot("").mobs[0].kills, 1);

        // Loot after pet kill must not double-count.
        eng.handle(
            parse_line(
                "[Thu Aug 06 21:51:22 2026] --You have looted a Bone Chip from a frenzied ghoul's corpse.--",
            ),
            &cfg,
        );
        assert_eq!(eng.snapshot("").mobs[0].kills, 1);
    }

    #[test]
    fn group_kill_then_loot_counts_once_via_loot() {
        let mut eng = eng_in("Lower Guk");
        let cfg = cfg_tracking();

        // Nearby group kill — not personal credit, ignored for denominator.
        assert!(!eng.handle(
            parse_line("[Thu Aug 06 21:51:20 2026] A frenzied ghoul has been slain by Vebn!",),
            &cfg,
        ));
        // First loot opens the opportunity.
        eng.handle(
            parse_line(
                "[Thu Aug 06 21:51:22 2026] --You have looted a Bone Chip from a frenzied ghoul's corpse.--",
            ),
            &cfg,
        );
        assert_eq!(eng.snapshot("").mobs[0].kills, 1);
    }

    #[test]
    fn rate_is_none_when_kills_zero() {
        // Snapshot edge: empty mob shouldn't divide by zero (kills always >= times path
        // after loot-only, but rate helper must stay safe).
        let eng = eng_in("Empty");
        let snap = eng.snapshot("");
        assert_eq!(snap.total_kills, 0);
        assert!(snap.items.is_empty());
    }
}
