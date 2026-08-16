//! Live combat meter: fight grouping, actor rows, ability breakdown, session stats.

use crate::engine::{looks_like_unnamed_npc, npc_names_match};
use crate::parser::LogEvent;
use crate::pets::{is_my_pet, is_pet_target};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use uuid::Uuid;

const FIGHT_TIMEOUT: Duration = Duration::from_secs(12);
const RECENT_CAP: usize = 50;
const EVENTS_CAP: usize = 4000;
const EMIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize)]
pub struct AbilityRow {
    pub name: String,
    pub kind: String,
    pub damage: u64,
    pub healing: u64,
    pub hits: u64,
    pub misses: u64,
    pub resists: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActorRow {
    pub key: String,
    pub name: String,
    pub is_you: bool,
    pub is_pet: bool,
    pub is_charm_pet: bool,
    pub damage: u64,
    pub healing: u64,
    pub taken: u64,
    pub dps: f64,
    pub hits: u64,
    pub misses: u64,
    pub resists: u64,
    pub abilities: Vec<AbilityRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FightHit {
    pub t_ms: i64,
    pub attacker: String,
    pub target: String,
    pub amount: u64,
    pub ability: String,
    pub kind: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FightSnapshot {
    pub id: String,
    pub title: String,
    pub zone: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_secs: f64,
    pub active: bool,
    pub damage: u64,
    pub healing: u64,
    pub dps: f64,
    pub actors: Vec<ActorRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub started_at: DateTime<Utc>,
    pub elapsed_secs: f64,
    pub kills: u64,
    pub kills_per_hour: f64,
    pub plat_copper: u64,
    pub plat_per_hour_copper: f64,
    pub deaths: u64,
    pub fights: u64,
    pub damage: u64,
    pub session_dps: f64,
    pub combat_secs: f64,
    pub zone: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeterSnapshot {
    pub character: String,
    pub zone: Option<String>,
    pub session: SessionSnapshot,
    pub current: Option<FightSnapshot>,
    pub overall: FightSnapshot,
    pub recent: Vec<FightSnapshot>,
}

#[derive(Debug, Clone)]
struct AbilityAcc {
    name: String,
    kind: String,
    damage: u64,
    healing: u64,
    hits: u64,
    misses: u64,
    resists: u64,
}

#[derive(Debug, Clone)]
struct ActorAcc {
    key: String,
    name: String,
    is_you: bool,
    is_pet: bool,
    is_charm_pet: bool,
    damage: u64,
    healing: u64,
    taken: u64,
    hits: u64,
    misses: u64,
    resists: u64,
    abilities: HashMap<String, AbilityAcc>,
}

#[derive(Debug, Clone)]
struct Fight {
    id: String,
    title: String,
    zone: Option<String>,
    started_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    actors: HashMap<String, ActorAcc>,
    hits: Vec<FightHit>,
    damage: u64,
    healing: u64,
}

pub struct MeterEngine {
    character_name: String,
    my_pet_name: String,
    zone: Option<String>,
    session_started: DateTime<Utc>,
    session_kills: u64,
    session_plat: u64,
    session_deaths: u64,
    session_fights: u64,
    session_damage: u64,
    session_combat_secs: f64,
    current: Option<Fight>,
    overall: Fight,
    recent: VecDeque<Fight>,
    last_emit: Instant,
}

impl MeterEngine {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            character_name: String::new(),
            my_pet_name: String::new(),
            zone: None,
            session_started: now,
            session_kills: 0,
            session_plat: 0,
            session_deaths: 0,
            session_fights: 0,
            session_damage: 0,
            session_combat_secs: 0.0,
            current: None,
            overall: Fight::new(now, None, "Overall"),
            recent: VecDeque::new(),
            last_emit: Instant::now()
                .checked_sub(EMIT_INTERVAL)
                .unwrap_or_else(Instant::now),
        }
    }

    pub fn set_identity(&mut self, character_name: &str, pet_name: &str) {
        self.character_name = character_name.trim().to_string();
        self.my_pet_name = pet_name.trim().to_string();
    }

    pub fn set_zone(&mut self, zone: &str) {
        let z = zone.trim();
        self.zone = if z.is_empty() {
            None
        } else {
            Some(z.to_string())
        };
    }

    pub fn current_zone(&self) -> Option<&str> {
        self.zone.as_deref()
    }

    pub fn has_active_fight(&self) -> bool {
        self.current.is_some()
    }

    pub fn should_emit(&self) -> bool {
        !self.has_active_fight() || self.last_emit.elapsed() >= EMIT_INTERVAL
    }

    pub fn mark_emitted(&mut self) {
        self.last_emit = Instant::now();
    }

    pub fn reset_session(&mut self) {
        let now = Utc::now();
        self.session_started = now;
        self.session_kills = 0;
        self.session_plat = 0;
        self.session_deaths = 0;
        self.session_fights = 0;
        self.session_damage = 0;
        self.session_combat_secs = 0.0;
        self.current = None;
        self.overall = Fight::new(now, self.zone.clone(), "Overall");
        self.recent.clear();
    }

    /// Close a stale fight. Returns true when a fight ended.
    pub fn tick(&mut self) -> bool {
        self.close_if_stale(Utc::now())
    }

    pub fn handle(&mut self, event: &LogEvent, charmed_targets: &[String]) -> bool {
        let now = Utc::now();
        match event {
            LogEvent::ZoneChange { zone } => {
                let closed = self.close_current(now);
                self.set_zone(zone);
                self.overall = Fight::new(now, self.zone.clone(), "Overall");
                closed
            }
            LogEvent::CorpseCoin { copper } => {
                self.session_plat = self.session_plat.saturating_add(*copper);
                true
            }
            LogEvent::Death {
                target,
                by_you,
                killer,
            } => {
                let mut changed = self.close_if_stale(now);
                if self.is_you_name(target) {
                    self.session_deaths = self.session_deaths.saturating_add(1);
                    changed = true;
                }
                if self.is_personal_kill(*by_you, killer.as_deref(), charmed_targets) {
                    self.session_kills = self.session_kills.saturating_add(1);
                    changed = true;
                }
                if let Some(fight) = self.current.as_mut() {
                    fight.last_activity = now;
                    if fight.title.eq_ignore_ascii_case("Combat")
                        || fight.title.to_ascii_lowercase().starts_with("a ")
                        || fight.title.to_ascii_lowercase().starts_with("an ")
                    {
                        fight.title = target.clone();
                    }
                }
                changed
            }
            LogEvent::CombatHit {
                attacker,
                target,
                amount,
                ability,
                kind,
                outcome,
                incoming,
            } => {
                self.close_if_stale(now);
                self.apply_hit(
                    now,
                    attacker,
                    target,
                    *amount,
                    ability,
                    kind,
                    outcome,
                    *incoming,
                    charmed_targets,
                );
                true
            }
            _ => self.close_if_stale(now),
        }
    }

    fn apply_hit(
        &mut self,
        now: DateTime<Utc>,
        attacker: &str,
        target: &str,
        amount: u64,
        ability: &str,
        kind: &str,
        outcome: &str,
        incoming: bool,
        charmed_targets: &[String],
    ) {
        let (actor_key, actor_name, is_you, is_pet, mut is_charm) =
            self.resolve_actor(attacker, charmed_targets);
        // A charm swinging at you is the break, not outgoing pet DPS.
        if incoming {
            is_charm = false;
        }
        let include_as_dps = is_you || is_pet || is_charm || !looks_like_unnamed_npc(&actor_name);

        if self.current.is_none() {
            let title = if incoming {
                attacker.to_string()
            } else {
                target.to_string()
            };
            self.current = Some(Fight::new(now, self.zone.clone(), &title));
        }

        let you_damage = is_you || is_pet || is_charm;
        if you_damage && kind != "heal" && outcome == "hit" && !incoming {
            self.session_damage = self.session_damage.saturating_add(amount);
        }

        let you_key = "you".to_string();
        let you_shown = if self.character_name.is_empty() {
            "You".to_string()
        } else {
            self.character_name.clone()
        };
        let credit_taken = incoming && kind != "heal" && outcome == "hit";
        let target_is_you = self.is_you_name(target);

        if let Some(fight) = self.current.as_mut() {
            fight.last_activity = now;
            let t_ms = now
                .signed_duration_since(fight.started_at)
                .num_milliseconds()
                .max(0);
            if fight.hits.len() < EVENTS_CAP {
                fight.hits.push(FightHit {
                    t_ms,
                    attacker: actor_name.clone(),
                    target: target.to_string(),
                    amount,
                    ability: ability.to_string(),
                    kind: kind.to_string(),
                    outcome: outcome.to_string(),
                });
            }
            if include_as_dps {
                record_on_fight(
                    fight,
                    &actor_key,
                    &actor_name,
                    is_you,
                    is_pet,
                    is_charm,
                    amount,
                    ability,
                    kind,
                    outcome,
                    incoming,
                    target_is_you,
                );
            }
            if credit_taken {
                credit_taken_on(fight, &you_key, &you_shown, amount);
            }
        }

        if include_as_dps {
            record_on_fight(
                &mut self.overall,
                &actor_key,
                &actor_name,
                is_you,
                is_pet,
                is_charm,
                amount,
                ability,
                kind,
                outcome,
                incoming,
                target_is_you,
            );
            self.overall.last_activity = now;
        }
        if credit_taken {
            credit_taken_on(&mut self.overall, &you_key, &you_shown, amount);
        }
    }

    fn resolve_actor(
        &self,
        raw: &str,
        charmed_targets: &[String],
    ) -> (String, String, bool, bool, bool) {
        let name = self.display_name(raw);
        if self.is_you_name(&name) {
            let shown = if self.character_name.is_empty() {
                "You".to_string()
            } else {
                self.character_name.clone()
            };
            return ("you".into(), shown, true, false, false);
        }
        if is_my_pet(&name, &self.my_pet_name)
            || name.eq_ignore_ascii_case("your pet")
            || name.eq_ignore_ascii_case("my pet")
        {
            let shown = if self.my_pet_name.is_empty() {
                "Your pet".to_string()
            } else {
                self.my_pet_name.clone()
            };
            return (
                format!("pet:{}", shown.to_ascii_lowercase()),
                shown,
                false,
                true,
                false,
            );
        }
        if charmed_targets.iter().any(|t| npc_names_match(t, &name)) {
            let shown = format!("{name} (charm)");
            return (
                format!("charm:{}", name.to_ascii_lowercase()),
                shown,
                false,
                false,
                true,
            );
        }
        let key = name.to_ascii_lowercase();
        let is_pet = is_pet_target(&name);
        (key, name, false, is_pet, false)
    }

    fn display_name(&self, raw: &str) -> String {
        let t = raw.trim();
        if t.eq_ignore_ascii_case("you") || t.eq_ignore_ascii_case("your") {
            if self.character_name.is_empty() {
                "You".into()
            } else {
                self.character_name.clone()
            }
        } else {
            t.to_string()
        }
    }

    fn is_you_name(&self, name: &str) -> bool {
        let n = name.trim();
        n.eq_ignore_ascii_case("you")
            || n.eq_ignore_ascii_case("your")
            || (!self.character_name.is_empty() && n.eq_ignore_ascii_case(&self.character_name))
    }

    fn is_personal_kill(
        &self,
        by_you: bool,
        killer: Option<&str>,
        charmed_targets: &[String],
    ) -> bool {
        if by_you {
            return true;
        }
        let Some(k) = killer else {
            return false;
        };
        if self.is_you_name(k) || is_my_pet(k, &self.my_pet_name) {
            return true;
        }
        charmed_targets.iter().any(|t| npc_names_match(t, k))
    }

    fn close_if_stale(&mut self, now: DateTime<Utc>) -> bool {
        let stale = self.current.as_ref().is_some_and(|f| {
            now.signed_duration_since(f.last_activity)
                .to_std()
                .unwrap_or(Duration::ZERO)
                >= FIGHT_TIMEOUT
        });
        if stale {
            self.close_current(now)
        } else {
            false
        }
    }

    fn close_current(&mut self, now: DateTime<Utc>) -> bool {
        let Some(mut fight) = self.current.take() else {
            return false;
        };
        fight.ended_at = Some(now);
        self.session_fights = self.session_fights.saturating_add(1);
        self.session_combat_secs += fight.duration_secs(now);
        self.recent.push_front(fight);
        while self.recent.len() > RECENT_CAP {
            self.recent.pop_back();
        }
        true
    }

    pub fn snapshot(&self) -> MeterSnapshot {
        let now = Utc::now();
        let elapsed = now
            .signed_duration_since(self.session_started)
            .to_std()
            .unwrap_or(Duration::ZERO)
            .as_secs_f64()
            .max(0.0);
        let hours = (elapsed / 3600.0).max(1.0 / 3600.0);
        let combat_secs = self.session_combat_secs
            + self
                .current
                .as_ref()
                .map(|f| f.duration_secs(now))
                .unwrap_or(0.0);
        MeterSnapshot {
            character: if self.character_name.is_empty() {
                "You".into()
            } else {
                self.character_name.clone()
            },
            zone: self.zone.clone(),
            session: SessionSnapshot {
                started_at: self.session_started,
                elapsed_secs: elapsed,
                kills: self.session_kills,
                kills_per_hour: self.session_kills as f64 / hours,
                plat_copper: self.session_plat,
                plat_per_hour_copper: self.session_plat as f64 / hours,
                deaths: self.session_deaths,
                fights: self.session_fights + if self.current.is_some() { 1 } else { 0 },
                damage: self.session_damage,
                session_dps: if elapsed > 0.0 {
                    self.session_damage as f64 / elapsed
                } else {
                    0.0
                },
                combat_secs,
                zone: self.zone.clone(),
            },
            current: self.current.as_ref().map(|f| f.snapshot(now, true)),
            overall: self.overall.snapshot(now, self.current.is_some()),
            recent: self.recent.iter().map(|f| f.snapshot(now, false)).collect(),
        }
    }
}

impl Fight {
    fn new(now: DateTime<Utc>, zone: Option<String>, title: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: title.to_string(),
            zone,
            started_at: now,
            last_activity: now,
            ended_at: None,
            actors: HashMap::new(),
            hits: Vec::new(),
            damage: 0,
            healing: 0,
        }
    }

    fn duration_secs(&self, now: DateTime<Utc>) -> f64 {
        let end = self.ended_at.unwrap_or(now);
        end.signed_duration_since(self.started_at)
            .to_std()
            .unwrap_or(Duration::ZERO)
            .as_secs_f64()
            .max(0.001)
    }

    fn snapshot(&self, now: DateTime<Utc>, active: bool) -> FightSnapshot {
        let duration = self.duration_secs(now);
        let mut actors: Vec<ActorRow> =
            self.actors.values().map(|a| a.snapshot(duration)).collect();
        actors.sort_by(|a, b| b.damage.cmp(&a.damage).then(a.name.cmp(&b.name)));
        FightSnapshot {
            id: self.id.clone(),
            title: self.title.clone(),
            zone: self.zone.clone(),
            started_at: self.started_at,
            ended_at: self.ended_at,
            duration_secs: duration,
            active,
            damage: self.damage,
            healing: self.healing,
            dps: self.damage as f64 / duration,
            actors,
        }
    }
}

impl ActorAcc {
    fn snapshot(&self, duration: f64) -> ActorRow {
        let mut abilities: Vec<AbilityRow> = self
            .abilities
            .values()
            .map(|a| AbilityRow {
                name: a.name.clone(),
                kind: a.kind.clone(),
                damage: a.damage,
                healing: a.healing,
                hits: a.hits,
                misses: a.misses,
                resists: a.resists,
            })
            .collect();
        abilities.sort_by(|a, b| b.damage.cmp(&a.damage).then(a.name.cmp(&b.name)));
        ActorRow {
            key: self.key.clone(),
            name: self.name.clone(),
            is_you: self.is_you,
            is_pet: self.is_pet,
            is_charm_pet: self.is_charm_pet,
            damage: self.damage,
            healing: self.healing,
            taken: self.taken,
            dps: self.damage as f64 / duration.max(0.001),
            hits: self.hits,
            misses: self.misses,
            resists: self.resists,
            abilities,
        }
    }
}

fn record_on_fight(
    fight: &mut Fight,
    key: &str,
    name: &str,
    is_you: bool,
    is_pet: bool,
    is_charm: bool,
    amount: u64,
    ability: &str,
    kind: &str,
    outcome: &str,
    incoming: bool,
    _target_is_you: bool,
) {
    let actor = fight
        .actors
        .entry(key.to_string())
        .or_insert_with(|| ActorAcc {
            key: key.to_string(),
            name: name.to_string(),
            is_you,
            is_pet,
            is_charm_pet: is_charm,
            damage: 0,
            healing: 0,
            taken: 0,
            hits: 0,
            misses: 0,
            resists: 0,
            abilities: HashMap::new(),
        });
    let ability_key = ability.to_ascii_lowercase();
    let acc = actor
        .abilities
        .entry(ability_key)
        .or_insert_with(|| AbilityAcc {
            name: ability.to_string(),
            kind: kind.to_string(),
            damage: 0,
            healing: 0,
            hits: 0,
            misses: 0,
            resists: 0,
        });

    match outcome {
        "resist" => {
            actor.resists = actor.resists.saturating_add(1);
            acc.resists = acc.resists.saturating_add(1);
        }
        "miss" | "dodge" | "parry" | "block" | "riposte" => {
            actor.misses = actor.misses.saturating_add(1);
            acc.misses = acc.misses.saturating_add(1);
        }
        _ => {
            actor.hits = actor.hits.saturating_add(1);
            acc.hits = acc.hits.saturating_add(1);
            if kind == "heal" {
                actor.healing = actor.healing.saturating_add(amount);
                acc.healing = acc.healing.saturating_add(amount);
                fight.healing = fight.healing.saturating_add(amount);
            } else if !incoming {
                actor.damage = actor.damage.saturating_add(amount);
                acc.damage = acc.damage.saturating_add(amount);
                fight.damage = fight.damage.saturating_add(amount);
            }
        }
    }
}

fn credit_taken_on(fight: &mut Fight, key: &str, name: &str, amount: u64) {
    let actor = fight
        .actors
        .entry(key.to_string())
        .or_insert_with(|| ActorAcc {
            key: key.to_string(),
            name: name.to_string(),
            is_you: true,
            is_pet: false,
            is_charm_pet: false,
            damage: 0,
            healing: 0,
            taken: 0,
            hits: 0,
            misses: 0,
            resists: 0,
            abilities: HashMap::new(),
        });
    actor.taken = actor.taken.saturating_add(amount);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_line;

    fn hit(line: &str) -> LogEvent {
        parse_line(line)
    }

    #[test]
    fn groups_hits_into_a_fight_and_session() {
        let mut m = MeterEngine::new();
        m.set_identity("Jungleberry", "");
        let e = hit("[Wed Aug 5 23:00:00 2026] You slash a gnoll for 28 points of damage.");
        assert!(m.handle(&e, &[]));
        let snap = m.snapshot();
        assert!(snap.current.is_some());
        let fight = snap.current.unwrap();
        assert_eq!(fight.damage, 28);
        assert!(fight.actors.iter().any(|a| a.is_you && a.damage == 28));
        assert_eq!(snap.session.damage, 28);
    }

    #[test]
    fn groupmate_gets_a_row() {
        let mut m = MeterEngine::new();
        m.set_identity("Jungleberry", "");
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] You slash a gnoll for 10 points of damage."),
            &[],
        );
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] Vebn hit a gnoll for 90 points of fire damage by Flame Shock."),
            &[],
        );
        let fight = m.snapshot().current.expect("fight");
        assert!(fight
            .actors
            .iter()
            .any(|a| a.name == "Vebn" && a.damage == 90));
        assert_eq!(fight.actors.len(), 2);
    }

    #[test]
    fn unnamed_npc_attacker_is_not_a_dps_row() {
        let mut m = MeterEngine::new();
        m.set_identity("Jungleberry", "");
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] A gnoll slashes YOU for 12 points of damage."),
            &[],
        );
        let fight = m.snapshot().current.expect("fight");
        assert!(fight.actors.is_empty() || fight.actors.iter().all(|a| a.damage == 0));
    }

    #[test]
    fn charm_pet_damage_is_attributed() {
        let mut m = MeterEngine::new();
        m.set_identity("Jungleberry", "");
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] an azarack slashes a gnoll for 40 points of damage."),
            &["an azarack".into()],
        );
        let fight = m.snapshot().current.expect("fight");
        let pet = fight
            .actors
            .iter()
            .find(|a| a.is_charm_pet)
            .expect("charm row");
        assert_eq!(pet.damage, 40);
        assert!(pet.name.contains("charm"));
    }

    #[test]
    fn charm_pet_matches_land_article_and_case() {
        let mut m = MeterEngine::new();
        m.set_identity("Jungleberry", "");
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] a gnoll slashes an orc pawn for 40 points of damage."),
            &["A gnoll".into()],
        );
        let fight = m.snapshot().current.expect("fight");
        let pet = fight
            .actors
            .iter()
            .find(|a| a.is_charm_pet)
            .expect("charm row");
        assert_eq!(pet.damage, 40);
    }

    #[test]
    fn charm_swinging_at_you_is_not_charm_dps() {
        let mut m = MeterEngine::new();
        m.set_identity("Jungleberry", "");
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] an azarack slashes YOU for 40 points of damage."),
            &["an azarack".into()],
        );
        let fight = m.snapshot().current.expect("fight");
        assert!(fight
            .actors
            .iter()
            .all(|a| !a.is_charm_pet || a.damage == 0));
        let you = fight.actors.iter().find(|a| a.is_you).expect("you");
        assert_eq!(you.taken, 40);
    }

    #[test]
    fn kill_and_coin_update_session() {
        let mut m = MeterEngine::new();
        m.set_identity("Jungleberry", "Gastik");
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] You have slain a gnoll!"),
            &[],
        );
        m.handle(
            &hit("[Wed Aug 5 23:00:01 2026] You receive 2 platinum from the corpse."),
            &[],
        );
        let s = m.snapshot().session;
        assert_eq!(s.kills, 1);
        assert_eq!(s.plat_copper, 2000);
    }

    #[test]
    fn zone_change_closes_fight_and_resets_overall() {
        let mut m = MeterEngine::new();
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] You slash a gnoll for 10 points of damage."),
            &[],
        );
        m.handle(
            &hit("[Wed Aug 5 23:00:01 2026] You have entered Lower Guk."),
            &[],
        );
        let snap = m.snapshot();
        assert!(snap.current.is_none());
        assert_eq!(snap.recent.len(), 1);
        assert_eq!(snap.overall.damage, 0);
        assert_eq!(snap.zone.as_deref(), Some("Lower Guk"));
    }

    #[test]
    fn combat_sample_fixture_parses_hits_and_session() {
        let raw = std::fs::read_to_string("../fixtures/combat_sample.log")
            .or_else(|_| std::fs::read_to_string("fixtures/combat_sample.log"))
            .expect("combat fixture");
        let mut m = MeterEngine::new();
        m.set_identity("Jungleberry", "Gastik");
        for line in raw.lines() {
            m.handle(&parse_line(line), &[]);
        }
        let snap = m.snapshot();
        assert!(snap.session.kills >= 1);
        assert!(snap.session.plat_copper > 0);
        let fight = snap
            .current
            .or(snap.recent.into_iter().next())
            .expect("fight");
        assert!(fight.actors.iter().any(|a| a.is_you && a.damage > 0));
        assert!(fight.actors.iter().any(|a| a.name == "Vebn"));
        assert!(fight.actors.iter().any(|a| a.is_pet));
    }

    #[test]
    fn reset_session_clears_totals() {
        let mut m = MeterEngine::new();
        m.handle(
            &hit("[Wed Aug 5 23:00:00 2026] You slash a gnoll for 10 points of damage."),
            &[],
        );
        m.reset_session();
        let s = m.snapshot().session;
        assert_eq!(s.damage, 0);
        assert_eq!(s.kills, 0);
        assert!(m.snapshot().current.is_none());
    }
}
