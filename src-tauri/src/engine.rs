use crate::parser::{extract_target, LogEvent};
use crate::spell_db::{
    duration_seconds, find_spell_by_name, is_watched, resolve_cast_spell, AppConfig, SpellDef,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTimer {
    pub id: String,
    pub spell: String,
    pub target: String,
    pub category: String,
    pub started_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub duration_secs: u64,
}

/// Timer that recently ended or was cleared; kept briefly for the overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentExpired {
    pub id: String,
    pub spell: String,
    pub target: String,
    pub category: String,
    pub ended_at: DateTime<Utc>,
}

/// Default recently-wore-off retention used by unit tests.
#[cfg(test)]
const DEFAULT_RECENT_TTL_SECS: u64 = 300;

fn recent_ttl_duration(secs: u64) -> chrono::Duration {
    let clamped = secs.clamp(
        crate::spell_db::RECENTLY_WORE_OFF_SECS_MIN,
        crate::spell_db::RECENTLY_WORE_OFF_SECS_MAX,
    );
    chrono::Duration::seconds(clamped as i64)
}

/// Enemy / detrimental-target timers: debuff, DoT, or lull/pacify line on someone
/// other than You. Shared with the overlay TS filter (and dual-window split).
///
/// Lulls are often `buff` in spells.json (EQ marks them Beneficial) but are cast
/// on NPCs — Calm, Soothe, Harmony, Pacify, Wake of Tranquility, etc.
#[allow(dead_code)]
pub fn is_enemy_timer(category: &str, target: &str, spell: &str) -> bool {
    if target.eq_ignore_ascii_case("You") {
        return false;
    }
    let cat = category.to_ascii_lowercase();
    cat == "debuff" || cat == "dot" || cat == "lull" || is_lull_spell(spell)
}

/// Self (`You`) or beneficial buffs on allies — shown on the main overlay when split.
#[allow(dead_code)]
pub fn is_friendly_timer(category: &str, target: &str, spell: &str) -> bool {
    !is_enemy_timer(category, target, spell)
}

/// Faction-drop / pacify line (often categorized `buff` because EQ marks Beneficial).
/// Name tokens cover the classic lull ladder plus AE variants; Evanescence is kept
/// for forward-compat even if absent from the current spells.json.
pub fn is_lull_spell(spell: &str) -> bool {
    let n = spell.to_ascii_lowercase();
    // Prefer specific phrases before short tokens.
    if n.contains("wake of tranquility") || n.contains("evanescence") {
        return true;
    }
    // Calm / Calming Visage / Calm Animal; Harmony / Harmony of Nature;
    // Lull / Lull Animal; Soothe; Pacify. "lull" also hits Lucid Lullaby (already debuff).
    const TOKENS: &[&str] = &["calm", "soothe", "lull", "pacify", "harmony"];
    TOKENS.iter().any(|t| n.contains(t))
}

/// Recently-wore-off list: renew-relevant self / ally buffs only.
///
/// Never enemy-targeted (debuff/DoT/lull on NPCs). Also never raw `debuff`/`dot`
/// even on You (drops self-roots like Ghoul Root). Name heuristics then drop
/// blossom/celestial heal HoTs, invis/camouflage, and root-line spells.
/// Regen-line buffs (Chloroplast, Regeneration, Regrowth, …) are kept so they
/// can be renewed from recently-wore-off.
pub fn should_record_recent(category: &str, target: &str, spell: &str) -> bool {
    if is_enemy_timer(category, target, spell) {
        return false;
    }
    let cat = category.to_ascii_lowercase();
    if cat == "debuff" || cat == "dot" {
        return false;
    }
    if !(target.eq_ignore_ascii_case("You") || cat == "buff") {
        return false;
    }
    !is_excluded_from_recent(spell)
}

/// Blossom/celestial heal HoTs, invis, and root spells excluded from recently-wore-off.
/// Regen-line buffs (Chloroplast / Regeneration / Regrowth) are intentionally allowed.
pub fn is_excluded_from_recent(spell: &str) -> bool {
    is_hot_spell(spell) || is_invis_spell(spell) || is_root_spell(spell)
}

fn is_hot_spell(spell: &str) -> bool {
    let n = spell.to_ascii_lowercase();
    // Plant / blooming heal line (Blossoming Heal, Efflorescing Heal, …).
    const PLANT_HOT: &[&str] = &[
        "blooming",
        "blossoming",
        "budding",
        "flowering",
        "sprouting",
        "efflorescing",
    ];
    if PLANT_HOT.iter().any(|p| n.contains(p)) {
        return true;
    }
    // Cleric celestial HoT line + any Remedy.
    if n.contains("remedy") {
        return true;
    }
    if n.starts_with("celestial ")
        && (n.contains("heal")
            || n.contains("health")
            || n.contains("cleansing")
            || n.contains("elixir"))
    {
        return true;
    }
    // Names ending in Heal / Healing (short combat HoTs; not HP buffs like Riotous Health).
    if n.ends_with(" heal") || n.ends_with(" healing") || n == "heal" || n == "healing" {
        return true;
    }
    false
}

fn is_invis_spell(spell: &str) -> bool {
    let n = spell.to_ascii_lowercase();
    // Utility buff — keep in recent list.
    if n.contains("see invis") {
        return false;
    }
    n.contains("invisib")
        || n.contains("camouflage")
        || n.contains("gather shadows")
        || n == "sunskin"
        || n.contains("skin of the shadow")
        // Stealth-style hide/veil; avoid matching unrelated names like "hideous".
        || n.split_whitespace().any(|w| w == "hide" || w == "hidden")
        || (n.contains("veil") && (n.contains("invis") || n.contains("shadow") || n.contains("camouflage")))
}

fn is_root_spell(spell: &str) -> bool {
    let n = spell.to_ascii_lowercase();
    n.contains("root") || n == "immobilize" || n.contains("immobilise")
}

/// After the first land from a pending cast, keep associating further lands with
/// the same spell/tier for this long (AE mez / multi-target land spam).
const AE_LAND_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
struct PendingCast {
    spell: String,
    /// Tier parsed from the cast line Roman numeral (0–10).
    tier: u32,
    started_at: DateTime<Utc>,
    /// Set when the first land_other matches this cast; starts the short AE window.
    first_land_at: Option<DateTime<Utc>>,
}

pub struct TimerEngine {
    pending: Option<PendingCast>,
    timers: Vec<ActiveTimer>,
    recent_expired: Vec<RecentExpired>,
    pending_timeout: Duration,
}

impl TimerEngine {
    pub fn new() -> Self {
        Self {
            pending: None,
            timers: Vec::new(),
            recent_expired: Vec::new(),
            pending_timeout: Duration::from_secs(15),
        }
    }

    pub fn timers(&self) -> &[ActiveTimer] {
        &self.timers
    }

    pub fn recent_expired(&self) -> &[RecentExpired] {
        &self.recent_expired
    }

    fn prune_recent(&mut self, recent_ttl_secs: u64) {
        let cutoff = Utc::now() - recent_ttl_duration(recent_ttl_secs);
        self.recent_expired.retain(|r| r.ended_at > cutoff);
    }

    fn record_ended(
        &mut self,
        removed: impl IntoIterator<Item = ActiveTimer>,
        recent_ttl_secs: u64,
    ) {
        let now = Utc::now();
        for t in removed {
            if !should_record_recent(&t.category, &t.target, &t.spell) {
                continue;
            }
            self.recent_expired.push(RecentExpired {
                id: t.id,
                spell: t.spell,
                target: t.target,
                category: t.category,
                ended_at: now,
            });
        }
        self.prune_recent(recent_ttl_secs);
    }

    /// Drop timers past `ends_at` into the recently-wore-off list.
    /// `recent_ttl_secs` is how long entries stay (clamped 15..=300).
    pub fn clear_expired(&mut self, recent_ttl_secs: u64) {
        let now = Utc::now();
        let (keep, expired): (Vec<_>, Vec<_>) =
            self.timers.drain(..).partition(|t| t.ends_at > now);
        self.timers = keep;
        if !expired.is_empty() {
            self.record_ended(expired, recent_ttl_secs);
        } else {
            self.prune_recent(recent_ttl_secs);
        }
    }

    pub fn handle(
        &mut self,
        event: LogEvent,
        spells: &[SpellDef],
        config: &AppConfig,
    ) -> bool {
        let recent_ttl_secs = config.overlay.recently_wore_off_secs_clamped();
        self.clear_expired(recent_ttl_secs);
        self.expire_stale_pending();

        let changed = match event {
            LogEvent::BeginCast { spell } => {
                if let Some((def, tier)) = resolve_cast_spell(spells, &spell) {
                    if is_watched(config, &def.name) {
                        self.pending = Some(PendingCast {
                            spell: def.name.clone(),
                            tier,
                            started_at: Utc::now(),
                            first_land_at: None,
                        });
                    }
                }
                false
            }
            LogEvent::Interrupted | LogEvent::Fizzle => {
                self.pending = None;
                false
            }
            LogEvent::LandOther { message, .. } => {
                // Wear-off lines can fall through to LandOther when heuristics miss;
                // check wear-off before treating the line as a land.
                if self.cancel_by_wear_off(&message, spells, recent_ttl_secs) {
                    true
                } else if self.try_land_you(&message, spells, config) {
                    // Self-buff land lines (e.g. "A cool breeze…") may not match LandYou
                    // heuristics; still try land_you before land_other.
                    true
                } else {
                    self.try_land_other(&message, spells, config)
                }
            }
            LogEvent::LandYou { message } => {
                if self.cancel_by_wear_off(&message, spells, recent_ttl_secs) {
                    true
                } else {
                    self.try_land_you(&message, spells, config)
                }
            }
            LogEvent::WearOff { message } => {
                if self.cancel_by_wear_off(&message, spells, recent_ttl_secs) {
                    true
                } else {
                    // Land lines that contain "fades" (Shade/Shadow/Umbra) used to be
                    // misclassified as wear-off; still try to start a timer.
                    self.try_land_you(&message, spells, config)
                        || self.try_land_other(&message, spells, config)
                }
            }
            LogEvent::MezBreak { target, .. } => self.cancel_by_target(&target, recent_ttl_secs),
            // Dead mobs lose every DoT/debuff; clear all timers for that name.
            LogEvent::Death { target, .. } => self.cancel_all_by_target(&target, recent_ttl_secs),
            LogEvent::ZoneChange { .. } => {
                // Keep timers across zones; only clear pending cast
                self.pending = None;
                false
            }
            // Level-ups are applied to AppConfig in lib.rs (duration formulas).
            LogEvent::LevelUp { .. } => false,
            LogEvent::LootItem { .. } | LogEvent::CorpseCoin { .. } => false,
            LogEvent::Other => false,
        };
        changed
    }

    fn expire_stale_pending(&mut self) {
        if let Some(p) = &self.pending {
            let now = Utc::now();
            let stale = if let Some(first) = p.first_land_at {
                // After the first AE land, only keep pending briefly for siblings.
                now.signed_duration_since(first)
                    .to_std()
                    .unwrap_or_default()
                    > AE_LAND_WINDOW
            } else {
                now.signed_duration_since(p.started_at)
                    .to_std()
                    .unwrap_or_default()
                    > self.pending_timeout
            };
            if stale {
                self.pending = None;
            }
        }
    }

    fn try_land_other(
        &mut self,
        message: &str,
        spells: &[SpellDef],
        config: &AppConfig,
    ) -> bool {
        let lower = message.to_lowercase();

        // Prefer matching against pending cast first
        if let Some(pending) = self.pending.clone() {
            if let Some(spell) = find_spell_by_name(spells, &pending.spell) {
                if let Some(target) = extract_target(message, &spell.land_other) {
                    if is_watched(config, &spell.name) {
                        self.start_timer(spell, &target, config, pending.tier);
                        // Keep pending for a short AE window so multi-target lands
                        // (Mesmerization, etc.) all get the same spell + tier.
                        // Clearing immediately made the 2nd+ target fall through to
                        // the first watched spell sharing the land text (e.g. Dazzle).
                        if let Some(p) = self.pending.as_mut() {
                            if p.first_land_at.is_none() {
                                p.first_land_at = Some(Utc::now());
                            }
                        }
                        return true;
                    }
                }
                // Self-buff land_you misclassified as LandOther (e.g. breeze lines)
                if !spell.land_you.is_empty()
                    && is_watched(config, &spell.name)
                    && lower.contains(&spell.land_you.to_lowercase())
                {
                    self.start_timer(spell, "You", config, pending.tier);
                    self.pending = None;
                    return true;
                }
            }
        }

        // Fallback: scan watched spells for land_other match (group member cast / missed begin).
        // Prefer the longest land_other phrase so "'s skin shimmers with divine power"
        // is not stolen by the shorter "'s skin shimmers" (Natureskin / PotG).
        let mut best_other: Option<(&SpellDef, String)> = None;
        for spell in spells {
            if !is_watched(config, &spell.name) || spell.land_other.is_empty() {
                continue;
            }
            if let Some(target) = extract_target(message, &spell.land_other) {
                let better = best_other
                    .as_ref()
                    .map(|(b, _)| spell.land_other.len() > b.land_other.len())
                    .unwrap_or(true);
                if better {
                    best_other = Some((spell, target));
                }
            }
        }
        if let Some((spell, target)) = best_other {
            self.start_timer(spell, &target, config, 0);
            self.pending = None;
            return true;
        }

        // Other caster buffed you, or land_you not caught by parser prefixes
        if let Some(spell) = best_watched_land_you(spells, config, &lower) {
            self.start_timer(spell, "You", config, 0);
            self.pending = None;
            return true;
        }
        false
    }

    fn try_land_you(
        &mut self,
        message: &str,
        spells: &[SpellDef],
        config: &AppConfig,
    ) -> bool {
        let lower = message.to_lowercase();

        if let Some(pending) = self.pending.clone() {
            if let Some(spell) = find_spell_by_name(spells, &pending.spell) {
                if !spell.land_you.is_empty()
                    && is_watched(config, &spell.name)
                    && lower.contains(&spell.land_you.to_lowercase())
                {
                    self.start_timer(spell, "You", config, pending.tier);
                    self.pending = None;
                    return true;
                }
            }
        }

        // Prefer longest land_you so Skin Like Nature is not stolen by
        // Natureskin / Protection of the Glades ("Your skin shimmers").
        if let Some(spell) = best_watched_land_you(spells, config, &lower) {
            self.start_timer(spell, "You", config, 0);
            self.pending = None;
            return true;
        }
        false
    }

    fn start_timer(&mut self, spell: &SpellDef, target: &str, config: &AppConfig, tier: u32) {
        let secs = duration_seconds(spell, config.character_level, tier);
        let now = Utc::now();
        let ends = now + chrono::Duration::seconds(secs as i64);

        // Self / ally buffs: reapply replaces the existing spell+target timer.
        // Enemy debuff/DoT/lull (CC, mez, pacify, etc.): always add — multiple NPCs
        // can share a name, so each land is an independent instance.
        if !is_enemy_timer(&spell.category, target, &spell.name) {
            self.timers.retain(|t| {
                !(t.spell.eq_ignore_ascii_case(&spell.name)
                    && t.target.eq_ignore_ascii_case(target))
            });
        }

        // Recast that lands again: drop matching recently-wore-off rows so the
        // active timer is not duplicated under "Recently wore off".
        self.recent_expired.retain(|r| {
            !(r.spell.eq_ignore_ascii_case(&spell.name) && r.target.eq_ignore_ascii_case(target))
        });

        self.timers.push(ActiveTimer {
            id: Uuid::new_v4().to_string(),
            spell: spell.name.clone(),
            target: target.to_string(),
            category: spell.category.clone(),
            started_at: now,
            ends_at: ends,
            duration_secs: secs,
        });
    }

    fn cancel_by_wear_off(
        &mut self,
        message: &str,
        spells: &[SpellDef],
        recent_ttl_secs: u64,
    ) -> bool {
        let msg = normalize_chat(message);
        if msg.is_empty() {
            return false;
        }

        // All spells whose wear_off_you matches this line (shared phrases like
        // "Your speed returns to normal" hit every haste with that text).
        let matching: Vec<&str> = spells
            .iter()
            .filter(|s| {
                let wear = normalize_chat(&s.wear_off_you);
                !wear.is_empty() && (msg.contains(&wear) || wear.contains(&msg))
            })
            .map(|s| s.name.as_str())
            .collect();

        if matching.is_empty() {
            return false;
        }

        // wear_off_you is always the player's own expire line — clear self timers
        // for every spell that lists this wear-off text.
        let (keep, removed): (Vec<_>, Vec<_>) = self.timers.drain(..).partition(|t| {
            let spell_hit = matching
                .iter()
                .any(|name| t.spell.eq_ignore_ascii_case(name));
            !(spell_hit && t.target.eq_ignore_ascii_case("You"))
        });
        self.timers = keep;
        let changed = !removed.is_empty();
        if changed {
            self.record_ended(removed, recent_ttl_secs);
        }
        changed
    }

    /// Remove a single timer for `target` (mez break only).
    ///
    /// When several NPCs share a name, the log cannot say which instance broke.
    /// We drop the matching timer closest to expiring (soonest `ends_at`), so one
    /// awaken does not clear every mez on that name.
    fn cancel_by_target(&mut self, target: &str, recent_ttl_secs: u64) -> bool {
        let mut best_idx: Option<usize> = None;
        for (i, t) in self.timers.iter().enumerate() {
            if !t.target.eq_ignore_ascii_case(target) {
                continue;
            }
            best_idx = Some(match best_idx {
                None => i,
                Some(j) if t.ends_at < self.timers[j].ends_at => i,
                Some(j) => j,
            });
        }
        let Some(idx) = best_idx else {
            return false;
        };
        let removed = self.timers.remove(idx);
        self.record_ended(std::iter::once(removed), recent_ttl_secs);
        true
    }

    /// Remove every timer for `target` (death). A slain mob loses all DoTs/debuffs.
    fn cancel_all_by_target(&mut self, target: &str, recent_ttl_secs: u64) -> bool {
        let mut removed = Vec::new();
        self.timers.retain(|t| {
            if t.target.eq_ignore_ascii_case(target) {
                removed.push(t.clone());
                false
            } else {
                true
            }
        });
        if removed.is_empty() {
            return false;
        }
        self.record_ended(removed, recent_ttl_secs);
        true
    }

    /// Remove an active timer without recording it in recently-wore-off
    /// (right-click dismiss is intentional, not a natural wear-off).
    /// Returns the removed timer when found.
    pub fn dismiss_timer(&mut self, id: &str) -> Option<ActiveTimer> {
        let mut removed = None;
        self.timers.retain(|t| {
            if t.id == id {
                removed = Some(t.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    pub fn clear_all(&mut self) {
        self.timers.clear();
        self.recent_expired.clear();
        self.pending = None;
    }

    #[cfg(test)]
    fn push_timer_for_test(&mut self, timer: ActiveTimer) {
        self.timers.push(timer);
    }
}

/// Lowercase + strip trailing punctuation so wear-off DB text matches log lines.
fn normalize_chat(s: &str) -> String {
    s.trim()
        .trim_end_matches(|c: char| c == '.' || c == '!' || c == '?')
        .trim()
        .to_lowercase()
}

/// Among watched spells whose `land_you` is contained in `lower_msg`, pick the
/// longest phrase (most specific match).
fn best_watched_land_you<'a>(
    spells: &'a [SpellDef],
    config: &AppConfig,
    lower_msg: &str,
) -> Option<&'a SpellDef> {
    let mut best: Option<&SpellDef> = None;
    for spell in spells {
        if spell.land_you.is_empty() || !is_watched(config, &spell.name) {
            continue;
        }
        let needle = spell.land_you.to_lowercase();
        if !lower_msg.contains(&needle) {
            continue;
        }
        let better = best
            .map(|b| spell.land_you.len() > b.land_you.len())
            .unwrap_or(true);
        if better {
            best = Some(spell);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_line;
    use crate::spell_db::load_spells;
    use std::fs;

    #[test]
    fn mez_cast_land_starts_timer() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 10;
        config.watched.insert("Mesmerize".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Wed Aug 5 23:00:00 2026] You begin casting Mesmerize."),
            &spells,
            &config,
        );
        let changed = engine.handle(
            parse_line("[Wed Aug 5 23:00:03 2026] A gnoll has been mesmerized."),
            &spells,
            &config,
        );
        assert!(changed);
        assert_eq!(engine.timers().len(), 1);
        assert_eq!(engine.timers()[0].spell, "Mesmerize");
        assert_eq!(engine.timers()[0].target, "A gnoll");
        assert_eq!(engine.timers()[0].duration_secs, 24);
    }

    #[test]
    fn same_name_mez_lands_stack_as_separate_timers() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 10;
        config.watched.insert("Mesmerize".into(), true);

        let mut engine = TimerEngine::new();
        for _ in 0..2 {
            engine.handle(
                parse_line("[Wed Aug 5 23:00:00 2026] You begin casting Mesmerize."),
                &spells,
                &config,
            );
            let changed = engine.handle(
                parse_line("[Wed Aug 5 23:00:03 2026] A gnoll has been mesmerized."),
                &spells,
                &config,
            );
            assert!(changed);
        }
        assert_eq!(engine.timers().len(), 2);
        assert!(engine
            .timers()
            .iter()
            .all(|t| t.spell == "Mesmerize" && t.target == "A gnoll"));
    }

    #[test]
    fn mez_break_removes_one_same_name_timer() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 10;
        config.watched.insert("Mesmerize".into(), true);

        let mut engine = TimerEngine::new();
        for _ in 0..2 {
            engine.handle(
                parse_line("[Wed Aug 5 23:00:00 2026] You begin casting Mesmerize."),
                &spells,
                &config,
            );
            engine.handle(
                parse_line("[Wed Aug 5 23:00:03 2026] A gnoll has been mesmerized."),
                &spells,
                &config,
            );
        }
        assert_eq!(engine.timers().len(), 2);

        let cleared = engine.handle(
            parse_line("[Wed Aug 5 23:00:10 2026] A gnoll has been awakened by Guard Beren."),
            &spells,
            &config,
        );
        assert!(cleared);
        assert_eq!(
            engine.timers().len(),
            1,
            "mez break should remove one instance, not all"
        );
        assert_eq!(engine.timers()[0].spell, "Mesmerize");
        assert_eq!(engine.timers()[0].target, "A gnoll");
    }

    #[test]
    fn death_clears_all_timers_for_target() {
        let mut engine = TimerEngine::new();
        let config = AppConfig::default();
        let now = Utc::now();
        engine.push_timer_for_test(ActiveTimer {
            id: "mez".into(),
            spell: "Mesmerize".into(),
            target: "A gnoll".into(),
            category: "debuff".into(),
            started_at: now - chrono::Duration::seconds(10),
            ends_at: now + chrono::Duration::seconds(14),
            duration_secs: 24,
        });
        engine.push_timer_for_test(ActiveTimer {
            id: "tepid".into(),
            spell: "Tepid Deeds".into(),
            target: "A gnoll".into(),
            category: "debuff".into(),
            started_at: now,
            ends_at: now + chrono::Duration::seconds(90),
            duration_secs: 90,
        });
        engine.push_timer_for_test(ActiveTimer {
            id: "other".into(),
            spell: "Tepid Deeds".into(),
            target: "An orc".into(),
            category: "debuff".into(),
            started_at: now,
            ends_at: now + chrono::Duration::seconds(90),
            duration_secs: 90,
        });

        let cleared = engine.handle(
            LogEvent::Death {
                target: "A gnoll".into(),
                by_you: true,
                killer: None,
            },
            &[],
            &config,
        );
        assert!(cleared);
        assert_eq!(engine.timers().len(), 1);
        assert_eq!(engine.timers()[0].id, "other");
    }

    #[test]
    fn self_buff_reapply_still_replaces() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 42;
        config.watched.insert("Clarity".into(), true);

        let mut engine = TimerEngine::new();
        for _ in 0..2 {
            engine.handle(
                parse_line("[Thu Aug 06 00:09:02 2026] You begin casting Clarity."),
                &spells,
                &config,
            );
            engine.handle(
                parse_line("[Thu Aug 06 00:09:03 2026] A cool breeze slips through your mind."),
                &spells,
                &config,
            );
        }
        assert_eq!(
            engine.timers().len(),
            1,
            "self buff reapply should replace, not stack"
        );
        assert_eq!(engine.timers()[0].target, "You");
    }

    #[test]
    fn cast_roman_tier_affects_duration() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 10;
        config.watched.insert("Mesmerize".into(), true);
        // Config tiers must be ignored — cast-line Roman numeral wins
        config.spell_tiers.insert("Mesmerize".into(), 0);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Wed Aug 5 23:00:00 2026] You begin casting Mesmerize V."),
            &spells,
            &config,
        );
        let changed = engine.handle(
            parse_line("[Wed Aug 5 23:00:03 2026] A gnoll has been mesmerized."),
            &spells,
            &config,
        );
        assert!(changed);
        assert_eq!(engine.timers().len(), 1);
        assert_eq!(engine.timers()[0].spell, "Mesmerize");
        // 4 ticks * (1 + 5*0.10) = 6 ticks = 36s
        assert_eq!(engine.timers()[0].duration_secs, 36);
    }

    /// AE Mesmerization lands on several NPCs in the same second. All must keep
    /// Mesmerization + cast-line tier — not fall through to Dazzle/Mesmerize.
    /// Repro from eqlog: Hoptor pet + bok ghoul knight after Mesmerization IV.
    #[test]
    fn ae_mez_multi_target_keeps_pending_spell_and_tier() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 50;
        // Dazzle is earlier in spells.json with the same land text; watching it
        // is what made the 2nd+ target get 360s before the AE pending window.
        config.watched.insert("Dazzle".into(), true);
        config.watched.insert("Mesmerize".into(), true);
        config.watched.insert("Mesmerization".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line(
                "[Wed Aug 05 21:41:26 2026] You begin casting Mesmerization IV.",
            ),
            &spells,
            &config,
        );
        for line in [
            "[Wed Aug 05 21:41:27 2026] Hoptor Thaggelum pet has been mesmerized.",
            "[Wed Aug 05 21:41:27 2026] a bok ghoul knight has been mesmerized.",
            "[Wed Aug 05 21:41:27 2026] Hoptor Thaggelum has been mesmerized.",
        ] {
            assert!(engine.handle(parse_line(line), &spells, &config));
        }

        assert_eq!(engine.timers().len(), 3);
        // Mesmerization: 4 ticks * (1 + 4*0.10) = 5.6 → 6 ticks = 36s
        for t in engine.timers() {
            assert_eq!(t.spell, "Mesmerization");
            assert_eq!(
                t.duration_secs, 36,
                "target {:?} should be Mesmerization IV (36s), not Dazzle/Mesmerize",
                t.target
            );
        }
        let targets: Vec<_> = engine.timers().iter().map(|t| t.target.as_str()).collect();
        assert_eq!(
            targets,
            vec![
                "Hoptor Thaggelum pet",
                "a bok ghoul knight",
                "Hoptor Thaggelum",
            ]
        );
    }

    #[test]
    fn fixture_log_produces_expected_timers() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 20;
        for spell in &spells {
            config.watched.insert(spell.name.clone(), true);
        }

        let path = fixture_path();
        let raw = fs::read_to_string(&path).unwrap_or_else(|_| {
            fs::read_to_string("../fixtures/sample_mez.log").expect("fixture")
        });

        let mut engine = TimerEngine::new();
        for line in raw.lines() {
            engine.handle(parse_line(line), &spells, &config);
        }

        let names: Vec<_> = engine
            .timers()
            .iter()
            .map(|t| (t.spell.as_str(), t.target.as_str()))
            .collect();

        // Mez broken by awaken; fizzle/interrupt dropped; Clarity + Root + Entrance remain
        assert!(names.contains(&("Clarity", "You")));
        assert!(names.contains(&("Root", "A decaying skeleton")));
        assert!(names.contains(&("Entrance", "An orc pawn")));
        assert!(!names.iter().any(|(s, _)| *s == "Mesmerize"));
        assert!(!names.iter().any(|(s, _)| *s == "Enthrall"));
    }

    #[test]
    fn clarity_jungleberry_log_starts_self_and_other_timers() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 42;
        config.watched.insert("Clarity".into(), true);

        let path = clarity_fixture_path();
        let raw = fs::read_to_string(&path).expect("clarity fixture");

        let mut engine = TimerEngine::new();
        for line in raw.lines() {
            engine.handle(parse_line(line), &spells, &config);
        }

        let names: Vec<_> = engine
            .timers()
            .iter()
            .map(|t| (t.spell.as_str(), t.target.as_str()))
            .collect();

        // Self-land wear-off cleared You; other-target Clarity on Vebn remains
        assert!(
            names.contains(&("Clarity", "Vebn")),
            "expected Clarity on Vebn, got {names:?}"
        );
        assert!(
            !names.iter().any(|(s, t)| *s == "Clarity" && *t == "You"),
            "self Clarity should wear off, got {names:?}"
        );
        // 270 ticks * 6s = 1620s at tier 0
        let vebn = engine
            .timers()
            .iter()
            .find(|t| t.target == "Vebn")
            .expect("Vebn timer");
        assert_eq!(vebn.duration_secs, 1620);
    }

    #[test]
    fn clarity_self_cast_starts_you_timer() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 42;
        config.watched.insert("Clarity".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Thu Aug 06 00:09:02 2026] You begin casting Clarity."),
            &spells,
            &config,
        );
        let changed = engine.handle(
            parse_line("[Thu Aug 06 00:09:03 2026] A cool breeze slips through your mind."),
            &spells,
            &config,
        );
        assert!(changed);
        assert_eq!(engine.timers().len(), 1);
        assert_eq!(engine.timers()[0].spell, "Clarity");
        assert_eq!(engine.timers()[0].target, "You");
        assert_eq!(engine.timers()[0].duration_secs, 1620);
    }

    #[test]
    fn wear_off_records_recent_expired() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 50;
        config.watched.insert("Celerity".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Thu Aug 06 01:00:00 2026] You begin casting Celerity."),
            &spells,
            &config,
        );
        engine.handle(
            parse_line("[Thu Aug 06 01:00:08 2026] You feel much faster."),
            &spells,
            &config,
        );
        assert_eq!(engine.timers().len(), 1);
        assert!(engine.recent_expired().is_empty());

        let cleared = engine.handle(
            parse_line("[Thu Aug 06 01:16:00 2026] Your speed returns to normal."),
            &spells,
            &config,
        );
        assert!(cleared);
        assert!(engine.timers().is_empty());
        assert_eq!(engine.recent_expired().len(), 1);
        assert_eq!(engine.recent_expired()[0].spell, "Celerity");
        assert_eq!(engine.recent_expired()[0].target, "You");
    }

    #[test]
    fn recast_purges_matching_recent_expired() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 50;
        config.watched.insert("Clarity".into(), true);
        config.watched.insert("Celerity".into(), true);

        let mut engine = TimerEngine::new();
        // Clarity lands, then wears off → recent list.
        engine.handle(
            parse_line("[Thu Aug 06 01:00:00 2026] You begin casting Clarity."),
            &spells,
            &config,
        );
        engine.handle(
            parse_line("[Thu Aug 06 01:00:08 2026] A cool breeze slips through your mind."),
            &spells,
            &config,
        );
        assert_eq!(engine.timers().len(), 1);
        engine.handle(
            parse_line("[Thu Aug 06 01:30:00 2026] The cool breeze fades."),
            &spells,
            &config,
        );
        assert!(engine.timers().is_empty());
        assert_eq!(engine.recent_expired().len(), 1);
        assert_eq!(engine.recent_expired()[0].spell, "Clarity");

        // Unrelated haste also in recent — should stay after Clarity recast.
        engine.push_timer_for_test(ActiveTimer {
            id: "haste".into(),
            spell: "Celerity".into(),
            target: "You".into(),
            category: "buff".into(),
            started_at: Utc::now() - chrono::Duration::seconds(10),
            ends_at: Utc::now() - chrono::Duration::seconds(1),
            duration_secs: 9,
        });
        engine.clear_expired(DEFAULT_RECENT_TTL_SECS);
        assert_eq!(engine.recent_expired().len(), 2);

        // Recast Clarity on You → Clarity drops from recent; Celerity stays.
        engine.handle(
            parse_line("[Thu Aug 06 01:31:00 2026] You begin casting Clarity."),
            &spells,
            &config,
        );
        engine.handle(
            parse_line("[Thu Aug 06 01:31:08 2026] A cool breeze slips through your mind."),
            &spells,
            &config,
        );
        assert_eq!(engine.timers().len(), 1);
        assert_eq!(engine.timers()[0].spell, "Clarity");
        assert_eq!(engine.recent_expired().len(), 1);
        assert_eq!(engine.recent_expired()[0].spell, "Celerity");
    }

    #[test]
    fn natural_expiry_excludes_enemy_debuff_from_recent() {
        let mut engine = TimerEngine::new();
        let past = Utc::now() - chrono::Duration::seconds(5);
        engine.push_timer_for_test(ActiveTimer {
            id: "t1".into(),
            spell: "Root".into(),
            target: "A gnoll".into(),
            category: "debuff".into(),
            started_at: past - chrono::Duration::seconds(30),
            ends_at: past,
            duration_secs: 30,
        });
        engine.clear_expired(DEFAULT_RECENT_TTL_SECS);
        assert!(engine.timers().is_empty());
        assert!(
            engine.recent_expired().is_empty(),
            "enemy debuff/DoT must not enter recently wore off"
        );
    }

    #[test]
    fn natural_expiry_records_buff_on_ally_as_recent() {
        let mut engine = TimerEngine::new();
        let past = Utc::now() - chrono::Duration::seconds(5);
        engine.push_timer_for_test(ActiveTimer {
            id: "t2".into(),
            spell: "Clarity".into(),
            target: "Vebn".into(),
            category: "buff".into(),
            started_at: past - chrono::Duration::seconds(30),
            ends_at: past,
            duration_secs: 30,
        });
        engine.clear_expired(DEFAULT_RECENT_TTL_SECS);
        assert!(engine.timers().is_empty());
        assert_eq!(engine.recent_expired().len(), 1);
        assert_eq!(engine.recent_expired()[0].spell, "Clarity");
        assert_eq!(engine.recent_expired()[0].target, "Vebn");
    }

    #[test]
    fn death_cancel_excludes_enemy_from_recent() {
        let mut engine = TimerEngine::new();
        let config = AppConfig::default();
        let now = Utc::now();
        engine.push_timer_for_test(ActiveTimer {
            id: "t3".into(),
            spell: "Tepid Deeds".into(),
            target: "A gnoll".into(),
            category: "debuff".into(),
            started_at: now,
            ends_at: now + chrono::Duration::seconds(60),
            duration_secs: 60,
        });
        let cleared = engine.handle(
            LogEvent::Death {
                target: "A gnoll".into(),
                by_you: true,
                killer: None,
            },
            &[],
            &config,
        );
        assert!(cleared);
        assert!(engine.timers().is_empty());
        assert!(engine.recent_expired().is_empty());
    }

    #[test]
    fn friend_enemy_classification() {
        assert!(is_friendly_timer("buff", "You", "Clarity"));
        assert!(is_friendly_timer("buff", "Jungleberry", "Spirit of Wolf"));
        assert!(is_friendly_timer("debuff", "You", "Ghoul Root"));
        assert!(is_enemy_timer("debuff", "A gnoll", "Tepid Deeds"));
        assert!(is_enemy_timer("dot", "An orc pawn", "Ignite Blood"));
        // Generic ally buff on another PC is still friendly.
        assert!(!is_enemy_timer("buff", "A gnoll", "Clarity"));
        // Lull/pacify line is buff in spells.json but enemy-targeted on mobs.
        assert!(is_enemy_timer("buff", "a frenzied ghoul", "Calm"));
        assert!(is_enemy_timer("buff", "a frenzied ghoul", "Harmony"));
        assert!(is_enemy_timer("buff", "a frenzied ghoul", "Soothe"));
        assert!(is_enemy_timer("buff", "a frenzied ghoul", "Lull"));
        assert!(is_enemy_timer("buff", "a frenzied ghoul", "Pacify"));
        assert!(is_enemy_timer("buff", "a frenzied ghoul", "Wake of Tranquility"));
        assert!(is_friendly_timer("buff", "You", "Clarity"));
        assert!(is_friendly_timer("buff", "You", "Calming Visage"));
        assert!(should_record_recent("buff", "Vebn", "Clarity"));
        // Enemy lulls must not enter recently wore off under the mob name.
        assert!(!should_record_recent("buff", "a frenzied ghoul", "Calm"));
        // Self-debuffs (e.g. Ghoul Root on You) must not enter recently wore off.
        assert!(!should_record_recent("debuff", "You", "Ghoul Root"));
        assert!(!should_record_recent("debuff", "A gnoll", "Root"));
        assert!(!should_record_recent("dot", "A gnoll", "Ignite Blood"));
        assert!(!should_record_recent("debuff", "A gnoll", "Tepid Deeds"));
    }

    #[test]
    fn recent_excludes_hot_invis_root_keeps_renew_buffs() {
        // Must NOT record
        assert!(!should_record_recent("debuff", "You", "Ghoul Root"));
        assert!(!should_record_recent("debuff", "You", "Ensnaring Roots"));
        assert!(!should_record_recent("buff", "You", "Blossoming Heal"));
        assert!(!should_record_recent("buff", "You", "Celestial Health"));
        assert!(!should_record_recent("debuff", "You", "Invisibility"));
        assert!(!should_record_recent("buff", "You", "Camouflage"));
        assert!(!should_record_recent("buff", "You", "Improved Invisibility"));

        // Must record — renew-relevant self / ally buffs (including regen line)
        assert!(should_record_recent("buff", "You", "Clarity"));
        assert!(should_record_recent("buff", "You", "Celerity"));
        assert!(should_record_recent("buff", "Vebn", "Clarity"));
        assert!(should_record_recent("buff", "You", "Spirit of Wolf"));
        assert!(should_record_recent("buff", "You", "Chloroplast"));
        assert!(should_record_recent("buff", "You", "Regeneration"));
        assert!(should_record_recent("buff", "You", "Regrowth"));
        assert!(should_record_recent("buff", "You", "Pack Chloroplast"));
        assert!(should_record_recent("buff", "You", "Extended Regeneration"));
    }

    #[test]
    fn natural_expiry_records_chloroplast_regen_as_recent() {
        let mut engine = TimerEngine::new();
        let past = Utc::now() - chrono::Duration::seconds(5);
        for spell in ["Chloroplast", "Regeneration", "Regrowth"] {
            engine.push_timer_for_test(ActiveTimer {
                id: spell.to_string(),
                spell: spell.into(),
                target: "You".into(),
                category: "buff".into(),
                started_at: past - chrono::Duration::seconds(30),
                ends_at: past,
                duration_secs: 30,
            });
        }
        engine.clear_expired(DEFAULT_RECENT_TTL_SECS);
        assert!(engine.timers().is_empty());
        let recent: Vec<_> = engine
            .recent_expired()
            .iter()
            .map(|r| r.spell.as_str())
            .collect();
        assert!(
            recent.contains(&"Chloroplast")
                && recent.contains(&"Regeneration")
                && recent.contains(&"Regrowth"),
            "regen-line buffs should record on natural expiry, got {recent:?}"
        );
    }

    #[test]
    fn recently_wore_off_ttl_respects_config_secs() {
        let mut engine = TimerEngine::new();
        let now = Utc::now();
        // Ended 45s ago — should survive a 60s TTL but not a 30s TTL.
        engine.recent_expired.push(RecentExpired {
            id: "r1".into(),
            spell: "Clarity".into(),
            target: "You".into(),
            category: "buff".into(),
            ended_at: now - chrono::Duration::seconds(45),
        });
        engine.clear_expired(60);
        assert_eq!(engine.recent_expired().len(), 1);
        engine.clear_expired(30);
        assert!(
            engine.recent_expired().is_empty(),
            "45s-old entry must drop when TTL is 30s"
        );
    }

    #[test]
    fn dismiss_timer_does_not_add_to_recent() {
        let mut engine = TimerEngine::new();
        let now = Utc::now();
        engine.push_timer_for_test(ActiveTimer {
            id: "t-dismiss".into(),
            spell: "Clarity".into(),
            target: "You".into(),
            category: "buff".into(),
            started_at: now,
            ends_at: now + chrono::Duration::seconds(60),
            duration_secs: 60,
        });
        assert!(engine.dismiss_timer("t-dismiss").is_some());
        assert!(engine.timers().is_empty());
        assert!(
            engine.recent_expired().is_empty(),
            "right-click dismiss must not push to recently wore off"
        );
    }

    #[test]
    fn natural_expiry_excludes_ghoul_root_blossoming_heal_invis() {
        let mut engine = TimerEngine::new();
        let past = Utc::now() - chrono::Duration::seconds(5);
        for (spell, target, category) in [
            ("Ghoul Root", "You", "debuff"),
            ("Blossoming Heal", "You", "buff"),
            ("Invisibility", "You", "debuff"),
            ("Clarity", "You", "buff"),
            ("Celerity", "You", "buff"),
        ] {
            engine.push_timer_for_test(ActiveTimer {
                id: spell.to_string(),
                spell: spell.into(),
                target: target.into(),
                category: category.into(),
                started_at: past - chrono::Duration::seconds(30),
                ends_at: past,
                duration_secs: 30,
            });
        }
        engine.clear_expired(DEFAULT_RECENT_TTL_SECS);
        assert!(engine.timers().is_empty());
        let recent: Vec<_> = engine
            .recent_expired()
            .iter()
            .map(|r| r.spell.as_str())
            .collect();
        assert!(
            !recent.contains(&"Ghoul Root"),
            "Ghoul Root must not enter recent, got {recent:?}"
        );
        assert!(
            !recent.contains(&"Blossoming Heal"),
            "Blossoming Heal must not enter recent, got {recent:?}"
        );
        assert!(
            !recent.contains(&"Invisibility"),
            "Invisibility must not enter recent, got {recent:?}"
        );
        assert!(
            recent.contains(&"Clarity") && recent.contains(&"Celerity"),
            "Clarity/Celerity on You should record, got {recent:?}"
        );
    }

    #[test]
    fn haste_wear_off_clears_celerity() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 50;
        config.watched.insert("Celerity".into(), true);

        let path = celerity_fixture_path();
        let raw = fs::read_to_string(&path).expect("celerity fixture");

        let mut engine = TimerEngine::new();
        for line in raw.lines() {
            engine.handle(parse_line(line), &spells, &config);
        }

        assert!(
            engine.timers().is_empty(),
            "Celerity should clear on shared haste wear-off, got {:?}",
            engine
                .timers()
                .iter()
                .map(|t| (t.spell.as_str(), t.target.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn shared_haste_wear_off_clears_all_matching_you_timers() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 50;
        config.watched.insert("Celerity".into(), true);
        config.watched.insert("Flurry".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Thu Aug 06 01:00:00 2026] You begin casting Celerity."),
            &spells,
            &config,
        );
        engine.handle(
            parse_line("[Thu Aug 06 01:00:08 2026] You feel much faster."),
            &spells,
            &config,
        );
        engine.handle(
            parse_line("[Thu Aug 06 01:00:10 2026] You begin casting Flurry."),
            &spells,
            &config,
        );
        engine.handle(
            parse_line("[Thu Aug 06 01:00:11 2026] You feel faster."),
            &spells,
            &config,
        );

        let names: Vec<_> = engine
            .timers()
            .iter()
            .filter(|t| t.target == "You")
            .map(|t| t.spell.as_str())
            .collect();
        assert!(
            names.contains(&"Celerity") && names.contains(&"Flurry"),
            "expected both Celerity and Flurry You timers, got {names:?}"
        );

        let cleared = engine.handle(
            parse_line("[Thu Aug 06 01:16:00 2026] Your speed returns to normal."),
            &spells,
            &config,
        );
        assert!(cleared);
        assert!(
            !engine.timers().iter().any(|t| {
                t.target == "You"
                    && (t.spell.eq_ignore_ascii_case("Celerity")
                        || t.spell.eq_ignore_ascii_case("Flurry"))
            }),
            "shared haste wear-off should clear all matching You timers, got {:?}",
            engine
                .timers()
                .iter()
                .map(|t| (t.spell.as_str(), t.target.as_str()))
                .collect::<Vec<_>>()
        );
    }

    fn fixture_path() -> std::path::PathBuf {
        let candidates = [
            std::path::PathBuf::from("fixtures/sample_mez.log"),
            std::path::PathBuf::from("../fixtures/sample_mez.log"),
            std::path::PathBuf::from("../../fixtures/sample_mez.log"),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| std::path::PathBuf::from("../fixtures/sample_mez.log"))
    }

    fn clarity_fixture_path() -> std::path::PathBuf {
        let candidates = [
            std::path::PathBuf::from("fixtures/clarity_jungleberry.log"),
            std::path::PathBuf::from("../fixtures/clarity_jungleberry.log"),
            std::path::PathBuf::from("../../fixtures/clarity_jungleberry.log"),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| {
                std::path::PathBuf::from("../fixtures/clarity_jungleberry.log")
            })
    }

    fn celerity_fixture_path() -> std::path::PathBuf {
        let candidates = [
            std::path::PathBuf::from("fixtures/celerity_wear_off.log"),
            std::path::PathBuf::from("../fixtures/celerity_wear_off.log"),
            std::path::PathBuf::from("../../fixtures/celerity_wear_off.log"),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| {
                std::path::PathBuf::from("../fixtures/celerity_wear_off.log")
            })
    }

    fn drifting_death_fixture_path() -> std::path::PathBuf {
        let candidates = [
            std::path::PathBuf::from("fixtures/drifting_death_hoptor.log"),
            std::path::PathBuf::from("../fixtures/drifting_death_hoptor.log"),
            std::path::PathBuf::from("../../fixtures/drifting_death_hoptor.log"),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| {
                std::path::PathBuf::from("../fixtures/drifting_death_hoptor.log")
            })
    }

    fn tepid_deeds_slain_fixture_path() -> std::path::PathBuf {
        let candidates = [
            std::path::PathBuf::from("fixtures/tepid_deeds_slain.log"),
            std::path::PathBuf::from("../fixtures/tepid_deeds_slain.log"),
            std::path::PathBuf::from("../../fixtures/tepid_deeds_slain.log"),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| {
                std::path::PathBuf::from("../fixtures/tepid_deeds_slain.log")
            })
    }

    /// EQL logs `engulfed by a swarm`; classic wiki often says `in`. Must still
    /// start a Drifting Death timer on Hoptor when watched.
    #[test]
    fn drifting_death_hoptor_starts_timer_from_eql_land() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 42;
        config.watched.insert("Drifting Death".into(), true);
        // Sibling DoT with the same land text — pending cast must win.
        config.watched.insert("Drones of Doom".into(), true);
        config.watched.insert("Creeping Crud".into(), true);

        let path = drifting_death_fixture_path();
        let raw = fs::read_to_string(&path).expect("drifting death fixture");

        let mut engine = TimerEngine::new();
        for line in raw.lines() {
            engine.handle(parse_line(line), &spells, &config);
        }

        assert_eq!(engine.timers().len(), 1, "expected one Drifting Death timer");
        let t = &engine.timers()[0];
        assert_eq!(t.spell, "Drifting Death");
        assert_eq!(t.target, "Hoptor Thaggelum");
        assert_eq!(t.duration_secs, 60); // 10 ticks fixed
    }

    /// EQL self-kill is `You have slain a frenzied ghoul!` (not `X has been slain by …`).
    /// After that line, no timers should remain for the slain target.
    #[test]
    fn tepid_deeds_cleared_on_you_have_slain() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 43;
        config.watched.insert("Tepid Deeds".into(), true);

        let path = tepid_deeds_slain_fixture_path();
        let raw = fs::read_to_string(&path).expect("tepid deeds slain fixture");

        let mut engine = TimerEngine::new();
        let mut saw_timer = false;
        for line in raw.lines() {
            engine.handle(parse_line(line), &spells, &config);
            if engine
                .timers()
                .iter()
                .any(|t| t.spell == "Tepid Deeds" && t.target.eq_ignore_ascii_case("a frenzied ghoul"))
            {
                saw_timer = true;
            }
        }

        assert!(saw_timer, "Tepid Deeds should land before the slain line");
        assert!(
            !engine
                .timers()
                .iter()
                .any(|t| t.target.eq_ignore_ascii_case("a frenzied ghoul")),
            "no timers should remain for the slain target"
        );
        assert!(engine.timers().is_empty());
    }

    /// Real Jungleberry lines: Shade land contains "fades" and used to be
    /// misparsed as WearOff, so no timer started.
    #[test]
    fn shade_self_and_ally_land_from_jungleberry_log() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.watched.insert("Shade".into(), true);

        let mut engine = TimerEngine::new();
        for line in [
            "[Sun Aug 09 00:20:15 2026] You begin casting Shade.",
            "[Sun Aug 09 00:20:17 2026] Your image fades.",
        ] {
            engine.handle(parse_line(line), &spells, &config);
        }
        assert!(
            engine
                .timers()
                .iter()
                .any(|t| t.spell == "Shade" && t.target == "You"),
            "expected Shade on You, got {:?}",
            engine
                .timers()
                .iter()
                .map(|t| (t.spell.as_str(), t.target.as_str()))
                .collect::<Vec<_>>()
        );

        engine.handle(
            parse_line("[Thu Aug 06 20:20:00 2026] You begin casting Shade."),
            &spells,
            &config,
        );
        engine.handle(
            parse_line("[Thu Aug 06 20:20:04 2026] Vebn's image fades around the edges."),
            &spells,
            &config,
        );
        assert!(
            engine
                .timers()
                .iter()
                .any(|t| t.spell == "Shade" && t.target == "Vebn"),
            "expected Shade on Vebn"
        );
    }

    /// Real Jungleberry lines: Skin like Nature self-land must not be stolen by
    /// shorter "Your skin shimmers" (Protection of the Glades / Natureskin).
    #[test]
    fn skin_like_nature_jungleberry_self_land() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 46;
        config.watched.insert("Skin Like Nature".into(), true);
        config.watched.insert("Protection of the Glades".into(), true);
        config.watched.insert("Natureskin".into(), true);
        config.watched.insert("Skin Like Diamond".into(), true);

        let mut engine = TimerEngine::new();
        for line in [
            "[Fri Aug 07 16:40:55 2026] You begin casting Skin like Nature.",
            "[Fri Aug 07 16:40:57 2026] Your skin returns to normal.",
            "[Fri Aug 07 16:40:57 2026] Your skin shimmers with divine power.",
            "[Fri Aug 07 16:40:57 2026] You healed Jungleberry for 53 (255) hit points by Skin like Nature.",
        ] {
            engine.handle(parse_line(line), &spells, &config);
        }

        assert_eq!(engine.timers().len(), 1, "expected Skin Like Nature on You");
        let t = &engine.timers()[0];
        assert_eq!(t.spell, "Skin Like Nature");
        assert_eq!(t.target, "You");
        // Wiki 1h12m24s → 724 ticks × 6s
        assert_eq!(t.duration_secs, 724 * 6);
    }

    /// Ally land without pending cast: longest land_other wins over PotG/Natureskin.
    #[test]
    fn skin_like_nature_ally_land_prefers_longest_phrase() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 46;
        config.watched.insert("Skin Like Nature".into(), true);
        config.watched.insert("Protection of the Glades".into(), true);
        config.watched.insert("Natureskin".into(), true);

        let mut engine = TimerEngine::new();
        let changed = engine.handle(
            parse_line(
                "[Fri Jul 17 20:36:22 2026] Faldimir's skin shimmers with divine power.",
            ),
            &spells,
            &config,
        );
        assert!(changed);
        assert_eq!(engine.timers().len(), 1);
        assert_eq!(engine.timers()[0].spell, "Skin Like Nature");
        assert_eq!(engine.timers()[0].target, "Faldimir");
    }

    /// Real Jungleberry cast+land: Gift of Magic (shared land text with Insight/
    /// Brilliance — pending cast must win). spells_us formula 3 cap 600 → 1h.
    #[test]
    fn gift_of_magic_jungleberry_self_cast_starts_timer() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 42;
        config.watched.insert("Gift of Magic".into(), true);
        config.watched.insert("Gift of Insight".into(), true);
        config.watched.insert("Gift of Brilliance".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Thu Jul 16 21:31:07 2026] You begin casting Gift of Magic."),
            &spells,
            &config,
        );
        let changed = engine.handle(
            parse_line(
                "[Thu Jul 16 21:31:12 2026] Your thoughts begin to race and flow faster.",
            ),
            &spells,
            &config,
        );
        assert!(changed);
        assert_eq!(engine.timers().len(), 1);
        let t = &engine.timers()[0];
        assert_eq!(t.spell, "Gift of Magic");
        assert_eq!(t.target, "You");
        assert_eq!(t.duration_secs, 600 * 6);
    }

    /// Ally land after pending Gift of Magic (same land_other as Insight/Brilliance).
    #[test]
    fn gift_of_magic_jungleberry_ally_land() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 42;
        config.watched.insert("Gift of Magic".into(), true);
        config.watched.insert("Gift of Insight".into(), true);
        config.watched.insert("Gift of Brilliance".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Thu Jul 16 21:36:48 2026] You begin casting Gift of Magic."),
            &spells,
            &config,
        );
        let changed = engine.handle(
            parse_line(
                "[Thu Jul 16 21:36:51 2026] Gastik appears to be staring into nothingness.",
            ),
            &spells,
            &config,
        );
        assert!(changed);
        assert_eq!(engine.timers().len(), 1);
        let t = &engine.timers()[0];
        assert_eq!(t.spell, "Gift of Magic");
        assert_eq!(t.target, "Gastik");
        assert_eq!(t.duration_secs, 600 * 6);
    }

    /// Cast name is bare "Shield of Thorns"; shared land text with Brambles/Spikes
    /// — pending cast must win or the wrong DS timer starts.
    #[test]
    fn shield_of_thorns_jungleberry_self_cast_starts_timer() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 47;
        // Mirrors real config: wiki "(Spell)" key watched, bare name false, and
        // sibling DS lines watched (would steal land without pending).
        config.watched.insert("Shield of Thorns".into(), false);
        config
            .watched
            .insert("Shield of Thorns (Spell)".into(), true);
        config.watched.insert("Shield of Brambles".into(), true);
        config.watched.insert("Shield of Spikes".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Thu Jul 16 21:07:20 2026] You begin casting Shield of Thorns."),
            &spells,
            &config,
        );
        let changed = engine.handle(
            parse_line(
                "[Thu Jul 16 21:07:21 2026] You are surrounded by a thorny barrier.",
            ),
            &spells,
            &config,
        );
        assert!(changed);
        assert_eq!(engine.timers().len(), 1);
        let t = &engine.timers()[0];
        assert_eq!(t.spell, "Shield of Thorns");
        assert_eq!(t.target, "You");
        assert_eq!(t.duration_secs, 150 * 6);
    }

    #[test]
    fn shield_of_thorns_jungleberry_ally_land() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 47;
        config
            .watched
            .insert("Shield of Thorns (Spell)".into(), true);
        config.watched.insert("Shield of Brambles".into(), true);
        config.watched.insert("Shield of Spikes".into(), true);

        let mut engine = TimerEngine::new();
        engine.handle(
            parse_line("[Thu Jul 16 22:03:07 2026] You begin casting Shield of Thorns."),
            &spells,
            &config,
        );
        let changed = engine.handle(
            parse_line(
                "[Thu Jul 16 22:03:08 2026] Gastik is surrounded by a thorny barrier.",
            ),
            &spells,
            &config,
        );
        assert!(changed);
        assert_eq!(engine.timers().len(), 1);
        let t = &engine.timers()[0];
        assert_eq!(t.spell, "Shield of Thorns");
        assert_eq!(t.target, "Gastik");
        assert_eq!(t.duration_secs, 150 * 6);
    }

    /// EQL uses "The symbol of Pinzarn flashes…", not classic wiki "A mystic symbol…".
    #[test]
    fn symbol_of_pinzarn_jungleberry_self_land() {
        let spells = load_spells().expect("spells");
        let mut config = AppConfig::default();
        config.character_level = 46;
        config.watched.insert("Symbol of Pinzarn".into(), true);
        config.watched.insert("Symbol of Ryltan".into(), true);
        config.watched.insert("Symbol of Transal".into(), true);

        let mut engine = TimerEngine::new();
        for line in [
            "[Fri Aug 07 16:41:03 2026] You begin casting Symbol of Pinzarn.",
            "[Fri Aug 07 16:41:05 2026] The mystic symbol fades.",
            "[Fri Aug 07 16:41:05 2026] The symbol of Pinzarn flashes before your eyes.",
            "[Fri Aug 07 16:41:05 2026] You healed Jungleberry for 152 (325) hit points by Symbol of Pinzarn.",
        ] {
            engine.handle(parse_line(line), &spells, &config);
        }

        assert_eq!(engine.timers().len(), 1, "expected Symbol of Pinzarn on You");
        let t = &engine.timers()[0];
        assert_eq!(t.spell, "Symbol of Pinzarn");
        assert_eq!(t.target, "You");
        assert_eq!(t.duration_secs, 450 * 6);
    }
}
