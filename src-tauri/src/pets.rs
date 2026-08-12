//! EQL pet target detection (see `data/pets.json`).
//!
//! Observed log forms:
//! - Other pets: `{OwnerName} pet` (e.g. `Hoptor Thaggelum pet`, `Miragul pet`)
//! - Your pet wear-off: `Your pet's {Spell} spell has worn off.`
//! - Your pet land/combat target: unique display name (e.g. `Gastik`) — set `my_pet_name`
//!
//! Overlay filtering mirrors this logic in `src/pets.ts`.

#![allow(dead_code)]

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct PetsFile {
    #[serde(default)]
    type_names: Vec<String>,
}

fn pets_file() -> &'static PetsFile {
    static FILE: OnceLock<PetsFile> = OnceLock::new();
    FILE.get_or_init(|| {
        let raw = include_str!("../../data/pets.json");
        serde_json::from_str(raw).unwrap_or(PetsFile {
            type_names: Vec::new(),
        })
    })
}

/// Known generic pet type / model names from `data/pets.json`.
pub fn pet_type_names() -> &'static [String] {
    &pets_file().type_names
}

/// True when `target` looks like an EQL pet (owner suffix or documented type name).
pub fn is_pet_target(target: &str) -> bool {
    is_pet_target_with(target, pet_type_names())
}

pub fn is_pet_target_with(target: &str, type_names: &[String]) -> bool {
    let t = target.trim();
    if t.is_empty() {
        return false;
    }
    if t.eq_ignore_ascii_case("You") {
        return false;
    }
    // Primary EQL pattern: "Hoptor Thaggelum pet", "a shadowknight pet"
    if t.len() >= 4 {
        let lower = t.to_ascii_lowercase();
        if lower.ends_with(" pet") {
            return true;
        }
    }
    type_names
        .iter()
        .any(|name| t.eq_ignore_ascii_case(name.trim()))
}

/// Exact match against the configured own-pet display name from land/combat lines.
pub fn is_my_pet(target: &str, my_pet_name: &str) -> bool {
    let mine = my_pet_name.trim();
    if mine.is_empty() {
        return false;
    }
    target.trim().eq_ignore_ascii_case(mine)
}

/// Main/friendly overlay visibility for a timer target given pet-related filters.
///
/// - `self_buffs_only`: keep You + my pet (enemies handled by caller).
/// - `hide_other_pets`: keep non-pets + my pet; drop other pets.
pub fn keep_friendly_target(
    target: &str,
    self_buffs_only: bool,
    hide_other_pets: bool,
    my_pet_name: &str,
) -> bool {
    keep_friendly_target_with(
        target,
        self_buffs_only,
        hide_other_pets,
        my_pet_name,
        pet_type_names(),
    )
}

pub fn keep_friendly_target_with(
    target: &str,
    self_buffs_only: bool,
    hide_other_pets: bool,
    my_pet_name: &str,
    type_names: &[String],
) -> bool {
    let mine = is_my_pet(target, my_pet_name);
    if self_buffs_only {
        if target.eq_ignore_ascii_case("You") || mine {
            // still apply hide_other_pets below (no-op for You / my pet)
        } else {
            return false;
        }
    }
    if hide_other_pets {
        let pet = mine || is_pet_target_with(target, type_names);
        if pet && !mine {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_owner_pet_suffix() {
        assert!(is_pet_target("Hoptor Thaggelum pet"));
        assert!(is_pet_target("Miragul pet"));
        assert!(is_pet_target("a shadowknight pet"));
        assert!(is_pet_target("Prince Bragnar pet"));
        assert!(is_pet_target("JUNGLEBERRY PET"));
        assert!(!is_pet_target("Hoptor Thaggelum"));
        assert!(!is_pet_target("You"));
        assert!(!is_pet_target("Gastik"));
        assert!(!is_pet_target("pet")); // bare word — needs leading name + space
    }

    #[test]
    fn detects_documented_type_names() {
        assert!(is_pet_target("an earth elemental"));
        assert!(is_pet_target("Spirit of Keshuval"));
        assert!(is_pet_target("a spirit guardian"));
        assert!(!is_pet_target("Spirit of Wolf")); // player buff, not a warder summon target
    }

    #[test]
    fn my_pet_exact_match() {
        assert!(is_my_pet("Gastik", "Gastik"));
        assert!(is_my_pet("gastik", "Gastik"));
        assert!(!is_my_pet("Gastik", ""));
        assert!(!is_my_pet("Hoptor Thaggelum pet", "Gastik"));
    }

    #[test]
    fn filter_self_buffs_allows_my_pet() {
        assert!(keep_friendly_target("You", true, false, "Gastik"));
        assert!(keep_friendly_target("Gastik", true, false, "Gastik"));
        assert!(!keep_friendly_target("Vebn", true, false, "Gastik"));
        assert!(!keep_friendly_target("Hoptor Thaggelum pet", true, false, "Gastik"));
    }

    #[test]
    fn filter_hide_other_pets_keeps_allies_and_mine() {
        assert!(keep_friendly_target("You", false, true, "Gastik"));
        assert!(keep_friendly_target("Gastik", false, true, "Gastik"));
        assert!(keep_friendly_target("Vebn", false, true, "Gastik"));
        assert!(!keep_friendly_target("Hoptor Thaggelum pet", false, true, "Gastik"));
        assert!(!keep_friendly_target("an earth elemental", false, true, "Gastik"));
        // Without my_pet_name, owner-suffix pets are hidden but allies remain.
        assert!(keep_friendly_target("Vebn", false, true, ""));
        assert!(!keep_friendly_target("Miragul pet", false, true, ""));
    }

    #[test]
    fn filter_combined_self_and_hide_pets() {
        assert!(keep_friendly_target("You", true, true, "Gastik"));
        assert!(keep_friendly_target("Gastik", true, true, "Gastik"));
        assert!(!keep_friendly_target("Vebn", true, true, "Gastik"));
        assert!(!keep_friendly_target("Miragul pet", true, true, "Gastik"));
    }

    #[test]
    fn pets_json_loads_type_names() {
        assert!(!pet_type_names().is_empty());
        assert!(pet_type_names()
            .iter()
            .any(|n| n.eq_ignore_ascii_case("an earth elemental")));
    }
}
