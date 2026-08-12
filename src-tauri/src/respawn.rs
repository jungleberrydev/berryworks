use crate::parser::LogEvent;
use crate::spawn_db::{find_zone, resolve_kill, CampsFile};
use crate::spell_db::AppConfig;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespawnTimer {
    pub id: String,
    pub zone_id: String,
    pub zone_name: String,
    pub label: String,
    pub npc_name: String,
    pub rare_id: Option<String>,
    pub is_rare: bool,
    pub started_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub duration_secs: u64,
}

pub struct RespawnEngine {
    current_zone: Option<String>,
    current_zone_id: Option<String>,
    timers: Vec<RespawnTimer>,
}

impl RespawnEngine {
    pub fn new() -> Self {
        Self {
            current_zone: None,
            current_zone_id: None,
            timers: Vec::new(),
        }
    }

    pub fn current_zone(&self) -> Option<&str> {
        self.current_zone.as_deref()
    }

    #[cfg(test)]
    pub fn timers(&self) -> &[RespawnTimer] {
        &self.timers
    }

    /// Timers visible for the respawn overlay: current zone only (others keep counting).
    pub fn visible_timers(&self) -> Vec<RespawnTimer> {
        let Some(zone) = &self.current_zone else {
            return Vec::new();
        };
        let zone_id = self.current_zone_id.as_deref();
        let mut list: Vec<_> = self
            .timers
            .iter()
            .filter(|t| {
                if let Some(zid) = zone_id {
                    if t.zone_id == zid {
                        return true;
                    }
                }
                t.zone_name.eq_ignore_ascii_case(zone)
            })
            .cloned()
            .collect();
        // Rares first, then soonest remaining.
        list.sort_by(|a, b| {
            b.is_rare
                .cmp(&a.is_rare)
                .then_with(|| a.ends_at.cmp(&b.ends_at))
        });
        list
    }

    pub fn clear_expired(&mut self) -> bool {
        let before = self.timers.len();
        let now = Utc::now();
        self.timers.retain(|t| t.ends_at > now);
        before != self.timers.len()
    }

    pub fn dismiss(&mut self, id: &str) -> bool {
        let before = self.timers.len();
        self.timers.retain(|t| t.id != id);
        before != self.timers.len()
    }

    pub fn clear_all(&mut self) {
        self.timers.clear();
    }

    /// Manually set (or clear) the active zone for kill matching + overlay filter.
    /// When `zone` matches a camps entry, uses that zone's canonical first name.
    pub fn set_zone(&mut self, zone: &str, camps: &CampsFile) -> bool {
        let trimmed = zone.trim();
        if trimmed.is_empty() {
            let changed = self.current_zone.is_some();
            self.current_zone = None;
            self.current_zone_id = None;
            return changed;
        }
        let (id, name) = if let Some(z) = find_zone(camps, trimmed) {
            (
                Some(z.id.clone()),
                z.names
                    .first()
                    .cloned()
                    .unwrap_or_else(|| trimmed.to_string()),
            )
        } else {
            (None, trimmed.to_string())
        };
        let changed = self
            .current_zone
            .as_ref()
            .map(|z| !z.eq_ignore_ascii_case(&name))
            .unwrap_or(true)
            || self.current_zone_id != id;
        self.current_zone = Some(name);
        self.current_zone_id = id;
        changed
    }

    pub fn handle(
        &mut self,
        event: LogEvent,
        camps: &CampsFile,
        config: &AppConfig,
    ) -> bool {
        match event {
            LogEvent::ZoneChange { zone } => self.set_zone(&zone, camps),
            LogEvent::Death { target } => {
                let Some(zone) = self.current_zone.clone() else {
                    return false;
                };
                let resolved = resolve_kill(camps, &zone, &target);

                if resolved.is_rare {
                    let rare_id = resolved.rare_id.as_deref().unwrap_or("");
                    let watched = config
                        .watched_rares
                        .get(rare_id)
                        .copied()
                        .unwrap_or(true);
                    if !watched {
                        return false;
                    }
                } else if !config.overlay.track_all_kills {
                    return false;
                }

                // Optional per-rare / zone override
                let mut secs = resolved.respawn_secs;
                if let Some(rid) = &resolved.rare_id {
                    if let Some(over) = config.camp_overrides.get(rid) {
                        secs = *over;
                    }
                } else if let Some(over) = config.camp_overrides.get(&resolved.zone_id) {
                    secs = *over;
                }

                if secs == 0 {
                    return false;
                }

                let now = Utc::now();
                let ends = now + chrono::Duration::seconds(secs as i64);
                self.timers.push(RespawnTimer {
                    id: Uuid::new_v4().to_string(),
                    zone_id: resolved.zone_id,
                    zone_name: resolved.zone_name,
                    label: resolved.label,
                    npc_name: resolved.npc_name,
                    rare_id: resolved.rare_id,
                    is_rare: resolved.is_rare,
                    started_at: now,
                    ends_at: ends,
                    duration_secs: secs,
                });
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_line;
    use crate::spawn_db::load_camps;
    use crate::spell_db::AppConfig;

    fn cfg_track_all() -> AppConfig {
        let mut c = AppConfig::default();
        c.overlay.track_all_kills = true;
        c.overlay.show_respawn_window = true;
        c
    }

    #[test]
    fn kill_in_zone_starts_timer() {
        let camps = load_camps().unwrap();
        let mut eng = RespawnEngine::new();
        let config = cfg_track_all();
        eng.handle(
            parse_line("[Thu Aug 7 12:00:00 2026] You have entered Lower Guk."),
            &camps,
            &config,
        );
        let changed = eng.handle(
            parse_line("[Thu Aug 7 12:01:00 2026] You have slain a froglok ghoul!"),
            &camps,
            &config,
        );
        assert!(changed);
        assert_eq!(eng.timers().len(), 1);
        assert_eq!(eng.timers()[0].duration_secs, 600);
        assert!(!eng.timers()[0].is_rare);
    }

    #[test]
    fn unknown_zone_uses_global_default() {
        let camps = load_camps().unwrap();
        let mut eng = RespawnEngine::new();
        let config = cfg_track_all();
        eng.handle(
            parse_line("[Thu Aug 7 12:00:00 2026] You have entered East Commonlands."),
            &camps,
            &config,
        );
        eng.handle(
            parse_line("[Thu Aug 7 12:01:00 2026] You have slain an orc pawn!"),
            &camps,
            &config,
        );
        assert_eq!(eng.timers()[0].duration_secs, 600);
        assert!(!eng.timers()[0].is_rare);
    }

    #[test]
    fn leave_zone_hides_but_keeps_timers() {
        let camps = load_camps().unwrap();
        let mut eng = RespawnEngine::new();
        let config = cfg_track_all();
        eng.handle(
            parse_line("[Thu Aug 7 12:00:00 2026] You have entered Lower Guk."),
            &camps,
            &config,
        );
        eng.handle(
            parse_line("[Thu Aug 7 12:01:00 2026] You have slain a froglok!"),
            &camps,
            &config,
        );
        eng.handle(
            parse_line("[Thu Aug 7 12:02:00 2026] You have entered East Commonlands."),
            &camps,
            &config,
        );
        assert_eq!(eng.timers().len(), 1);
        assert!(eng.visible_timers().is_empty());
        eng.handle(
            parse_line("[Thu Aug 7 12:03:00 2026] You have entered The Ruins of Old Guk."),
            &camps,
            &config,
        );
        assert_eq!(eng.visible_timers().len(), 1);
    }

    #[test]
    fn manual_set_zone_enables_kills() {
        let camps = load_camps().unwrap();
        let mut eng = RespawnEngine::new();
        let config = cfg_track_all();
        assert!(eng.set_zone("Lower Guk", &camps));
        let changed = eng.handle(
            parse_line("[Thu Aug 7 12:01:00 2026] You have slain a froglok!"),
            &camps,
            &config,
        );
        assert!(changed);
        assert_eq!(eng.visible_timers().len(), 1);
    }

    #[test]
    fn same_name_stacks() {
        let camps = load_camps().unwrap();
        let mut eng = RespawnEngine::new();
        let config = cfg_track_all();
        eng.handle(
            parse_line("[Thu Aug 7 12:00:00 2026] You have entered Lower Guk."),
            &camps,
            &config,
        );
        eng.handle(
            parse_line("[Thu Aug 7 12:01:00 2026] You have slain a froglok!"),
            &camps,
            &config,
        );
        eng.handle(
            parse_line("[Thu Aug 7 12:01:30 2026] You have slain a froglok!"),
            &camps,
            &config,
        );
        assert_eq!(eng.timers().len(), 2);
    }
}
