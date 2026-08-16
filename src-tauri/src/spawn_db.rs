use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RareCamp {
    pub id: String,
    pub label: String,
    pub npc_names: Vec<String>,
    pub respawn_secs: u64,
    #[serde(default)]
    pub watched_by_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneCamps {
    pub id: String,
    /// Names as they appear in `You have entered …`
    pub names: Vec<String>,
    pub default_respawn_secs: u64,
    #[serde(default)]
    pub rares: Vec<RareCamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampsFile {
    #[serde(default = "default_global")]
    pub global_default_respawn_secs: u64,
    pub zones: Vec<ZoneCamps>,
}

fn default_global() -> u64 {
    400
}

pub fn load_camps() -> Result<CampsFile, String> {
    let candidates = [
        PathBuf::from("data/camps.json"),
        PathBuf::from("../data/camps.json"),
        resource_camps_path(),
    ];
    for path in candidates {
        if path.exists() {
            let raw = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            return serde_json::from_str(&raw).map_err(|e| format!("Invalid camps.json: {e}"));
        }
    }
    let raw = include_str!("../../data/camps.json");
    serde_json::from_str(raw).map_err(|e| format!("Invalid embedded camps.json: {e}"))
}

fn resource_camps_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("resources").join("camps.json");
        }
    }
    PathBuf::from("resources/camps.json")
}

pub fn find_zone<'a>(camps: &'a CampsFile, zone_name: &str) -> Option<&'a ZoneCamps> {
    let needle = zone_name.trim().to_lowercase();
    camps.zones.iter().find(|z| {
        z.names
            .iter()
            .any(|n| n.trim().eq_ignore_ascii_case(&needle))
    })
}

/// Resolve respawn seconds + optional rare metadata for a kill in a zone.
pub fn resolve_kill(camps: &CampsFile, zone_name: &str, npc_name: &str) -> KillResolve {
    let zone = find_zone(camps, zone_name);
    let default_secs = zone
        .map(|z| z.default_respawn_secs)
        .unwrap_or(camps.global_default_respawn_secs);

    if let Some(z) = zone {
        for rare in &z.rares {
            if rare
                .npc_names
                .iter()
                .any(|n| n.trim().eq_ignore_ascii_case(npc_name.trim()))
            {
                return KillResolve {
                    zone_id: z.id.clone(),
                    zone_name: zone_name.to_string(),
                    label: rare.label.clone(),
                    npc_name: npc_name.to_string(),
                    respawn_secs: rare.respawn_secs,
                    rare_id: Some(rare.id.clone()),
                    is_rare: true,
                };
            }
        }
        return KillResolve {
            zone_id: z.id.clone(),
            zone_name: zone_name.to_string(),
            label: npc_name.to_string(),
            npc_name: npc_name.to_string(),
            respawn_secs: default_secs,
            rare_id: None,
            is_rare: false,
        };
    }

    KillResolve {
        zone_id: "_unknown".into(),
        zone_name: zone_name.to_string(),
        label: npc_name.to_string(),
        npc_name: npc_name.to_string(),
        respawn_secs: default_secs,
        rare_id: None,
        is_rare: false,
    }
}

#[derive(Debug, Clone)]
pub struct KillResolve {
    pub zone_id: String,
    pub zone_name: String,
    pub label: String,
    pub npc_name: String,
    pub respawn_secs: u64,
    pub rare_id: Option<String>,
    pub is_rare: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lower_guk_ten_minute_default() {
        let camps = load_camps().expect("camps");
        let z = find_zone(&camps, "Lower Guk").expect("zone");
        assert_eq!(z.default_respawn_secs, 600);
        let alt = find_zone(&camps, "The Ruins of Old Guk").expect("alt name");
        assert_eq!(alt.id, "lower-guk");
        let trash = resolve_kill(&camps, "Lower Guk", "a froglok ghoul");
        assert!(!trash.is_rare);
        assert_eq!(trash.respawn_secs, 600);
        assert!(find_zone(&camps, "Blackburrow").is_none());
    }
}
