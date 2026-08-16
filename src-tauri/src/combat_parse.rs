//! Combat / heal / resist / miss lines from the EverQuest Legends log.
//!
//! Matched from `parse_line` before wear-off / LandYou / LandOther so DoT ticks
//! and heals are not treated as spell-land text.

use crate::parser::LogEvent;
use regex::Regex;
use std::sync::OnceLock;

fn re_damage_shield() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<victim>.+?) (?:is|are) (?:pierced|burned|tormented|chilled|frozen|engulfed|blasted|hit) by (?:YOUR|(?P<owner>.+?)(?:'s|`s|’s)) (?P<style>\w+) for (?P<amount>\d+) points? of non-melee damage!?\s*\.?\s*(?:\((?P<mods>.+)\))?\s*$",
        )
        .unwrap()
    })
}

fn re_frenzy_on() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<attacker>.+?) frenzy on (?P<target>.+?) for (?P<amount>\d+) points? of damage\s*(?:\((?P<mods>.+)\))?\s*\.?\s*$",
        )
        .unwrap()
    })
}

fn re_spell_by() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<attacker>.+?) hit (?P<target>.+?) for (?P<amount>\d+) points? of (?P<resist>\w+) damage by (?P<spell>.+?)\s*(?:\((?P<mods>.+)\))?\s*\.?\s*$",
        )
        .unwrap()
    })
}

fn re_non_melee() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<attacker>.+?) hit (?P<target>.+?) for (?P<amount>\d+) points? of non-melee damage\s*(?:\((?P<mods>.+)\))?\s*\.?\s*$",
        )
        .unwrap()
    })
}

fn re_melee() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<attacker>.+?) (?P<verb>hit|hits|slash|slashes|crush|crushes|pierce|pierces|kick|kicks|bash|bashes|strike|strikes|claw|claws|bite|bites|punch|punches|backstab|backstabs|smite|smites|cleave|cleaves|frenzy|frenzies|maul|mauls|shoot|shoots|slam|slams) (?P<target>.+?) for (?P<amount>\d+) points? of damage\s*(?:\((?P<mods>.+)\))?\s*\.?\s*$",
        )
        .unwrap()
    })
}

fn re_possessive_spell() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<attacker>.+?)(?:'s|`s|’s) (?P<spell>.+?) (?P<verb>hit|hits) (?P<target>.+?) for (?P<amount>\d+) points? of (?:non-melee )?damage\s*(?:\((?P<mods>.+)\))?\s*\.?\s*$",
        )
        .unwrap()
    })
}

fn re_your_dot() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<target>.+?) has taken (?P<amount>\d+) damage from your (?P<spell>.+?)\.?\s*$",
        )
        .unwrap()
    })
}

fn re_dot_from_by() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<target>.+?) has taken (?P<amount>\d+) damage from (?P<spell>.+?) by (?P<attacker>.+?)\.?\s*$",
        )
        .unwrap()
    })
}

fn re_dot_by() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<target>.+?) has taken (?P<amount>\d+) damage by (?P<spell>.+?)\.?\s*$",
        )
        .unwrap()
    })
}

fn re_you_taken_dot() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^You have taken (?P<amount>\d+) damage from (?P<spell>.+?)(?: by (?P<attacker>.+?))?\.?\s*$",
        )
        .unwrap()
    })
}

fn re_you_healed() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^You have been healed for (?P<amount>\d+) (?:hit )?points?\.?\s*$")
            .unwrap()
    })
}

fn re_other_healed() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<target>.+?) has been healed for (?P<amount>\d+) (?:hit )?points?(?: by (?P<source>.+?))?\.?\s*$",
        )
        .unwrap()
    })
}

fn re_resist_your() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(?P<target>.+?) resisted your (?P<spell>.+?)!?\s*$").unwrap()
    })
}

fn re_you_resist() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^You resist(?:ed)? (?:the )?(?P<spell>.+?)(?: spell)?!?\s*$").unwrap()
    })
}

fn re_resist_other() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<target>.+?) resisted (?P<attacker>.+?)(?:'s|`s|’s) (?P<spell>.+?)!?\s*$",
        )
        .unwrap()
    })
}

fn re_miss() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?P<attacker>.+?) (?:try|tries) to (?P<verb>\w+) (?P<target>.+?), but (?P<rest>.+)$",
        )
        .unwrap()
    })
}

fn hit(
    attacker: String,
    target: String,
    amount: u64,
    ability: String,
    kind: &str,
    outcome: &str,
) -> LogEvent {
    let incoming = is_you(&target);
    LogEvent::CombatHit {
        attacker,
        target,
        amount,
        ability,
        kind: kind.to_string(),
        outcome: outcome.to_string(),
        incoming,
    }
}

fn is_you(name: &str) -> bool {
    name.eq_ignore_ascii_case("you") || name.eq_ignore_ascii_case("your")
}

fn normalize_hit_type(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    match lower.as_str() {
        "hits" => "hit".into(),
        "slashes" => "slash".into(),
        "crushes" => "crush".into(),
        "pierces" => "pierce".into(),
        "kicks" => "kick".into(),
        "bashes" => "bash".into(),
        "strikes" => "strike".into(),
        "claws" => "claw".into(),
        "bites" => "bite".into(),
        "punches" => "punch".into(),
        "backstabs" => "backstab".into(),
        "smites" => "smite".into(),
        "cleaves" => "cleave".into(),
        "frenzies" => "frenzy".into(),
        "mauls" => "maul".into(),
        "shoots" => "shoot".into(),
        "slams" => "slam".into(),
        other => other.to_string(),
    }
}

fn miss_outcome(rest: &str) -> Option<&'static str> {
    let r = rest.to_ascii_lowercase();
    if r.contains("dodge") {
        Some("dodge")
    } else if r.contains("parry") || r.contains("parries") {
        Some("parry")
    } else if r.contains("block") {
        Some("block")
    } else if r.contains("riposte") {
        Some("riposte")
    } else if r.contains("miss") {
        Some("miss")
    } else {
        None
    }
}

/// Parse a timestamp-stripped combat line, or `None` if it is not combat.
pub fn parse_combat_line(msg: &str) -> Option<LogEvent> {
    if let Some(caps) = re_damage_shield().captures(msg) {
        let owner = caps
            .name("owner")
            .map(|m| m.as_str().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "You".to_string());
        let style = caps["style"].to_string();
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            owner,
            caps["victim"].trim().to_string(),
            amount,
            format!("Damage Shield ({style})"),
            "ds",
            "hit",
        ));
    }

    if let Some(caps) = re_frenzy_on().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            caps["attacker"].trim().to_string(),
            caps["target"].trim().to_string(),
            amount,
            "frenzy".into(),
            "melee",
            "hit",
        ));
    }

    if let Some(caps) = re_spell_by().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            caps["attacker"].trim().to_string(),
            caps["target"].trim().to_string(),
            amount,
            caps["spell"].trim().trim_end_matches('.').to_string(),
            "spell",
            "hit",
        ));
    }

    if let Some(caps) = re_you_healed().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            "You".into(),
            "You".into(),
            amount,
            "Heal".into(),
            "heal",
            "hit",
        ));
    }

    if let Some(caps) = re_other_healed().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        let source = caps
            .name("source")
            .map(|m| m.as_str().trim().to_string())
            .filter(|s| !s.is_empty());
        let (attacker, ability) = match source {
            Some(s) if s.to_ascii_lowercase().starts_with("your ") => {
                ("You".into(), s[5..].trim().to_string())
            }
            Some(s) => (s, "Heal".into()),
            None => ("You".into(), "Heal".into()),
        };
        return Some(hit(
            attacker,
            caps["target"].trim().to_string(),
            amount,
            ability,
            "heal",
            "hit",
        ));
    }

    if let Some(caps) = re_resist_your().captures(msg) {
        return Some(hit(
            "You".into(),
            caps["target"].trim().to_string(),
            0,
            caps["spell"].trim().trim_end_matches('.').to_string(),
            "spell",
            "resist",
        ));
    }

    if let Some(caps) = re_you_resist().captures(msg) {
        return Some(hit(
            "Unknown".into(),
            "You".into(),
            0,
            caps["spell"].trim().trim_end_matches('.').to_string(),
            "spell",
            "resist",
        ));
    }

    if let Some(caps) = re_resist_other().captures(msg) {
        return Some(hit(
            caps["attacker"].trim().to_string(),
            caps["target"].trim().to_string(),
            0,
            caps["spell"].trim().trim_end_matches('.').to_string(),
            "spell",
            "resist",
        ));
    }

    if let Some(caps) = re_miss().captures(msg) {
        let outcome = miss_outcome(&caps["rest"])?;
        let ability = normalize_hit_type(caps["verb"].trim());
        return Some(hit(
            caps["attacker"].trim().to_string(),
            caps["target"].trim().to_string(),
            0,
            ability,
            "melee",
            outcome,
        ));
    }

    if let Some(caps) = re_melee().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        let ability = normalize_hit_type(caps["verb"].trim());
        return Some(hit(
            caps["attacker"].trim().to_string(),
            caps["target"].trim().to_string(),
            amount,
            ability,
            "melee",
            "hit",
        ));
    }

    if let Some(caps) = re_non_melee().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            caps["attacker"].trim().to_string(),
            caps["target"].trim().to_string(),
            amount,
            "non-melee".into(),
            "spell",
            "hit",
        ));
    }

    if let Some(caps) = re_possessive_spell().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            caps["attacker"].trim().to_string(),
            caps["target"].trim().to_string(),
            amount,
            caps["spell"].trim().to_string(),
            "spell",
            "hit",
        ));
    }

    if let Some(caps) = re_you_taken_dot().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        let attacker = caps
            .name("attacker")
            .map(|m| m.as_str().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown".to_string());
        return Some(hit(
            attacker,
            "You".into(),
            amount,
            caps["spell"].trim().trim_end_matches('.').to_string(),
            "dot",
            "hit",
        ));
    }

    if let Some(caps) = re_dot_from_by().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            caps["attacker"].trim().to_string(),
            caps["target"].trim().to_string(),
            amount,
            caps["spell"].trim().to_string(),
            "dot",
            "hit",
        ));
    }

    if let Some(caps) = re_your_dot().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            "You".into(),
            caps["target"].trim().to_string(),
            amount,
            caps["spell"].trim().trim_end_matches('.').to_string(),
            "dot",
            "hit",
        ));
    }

    if let Some(caps) = re_dot_by().captures(msg) {
        let amount: u64 = caps["amount"].parse().ok()?;
        return Some(hit(
            "Unknown".into(),
            caps["target"].trim().to_string(),
            amount,
            caps["spell"].trim().trim_end_matches('.').to_string(),
            "dot",
            "hit",
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combat(msg: &str) -> LogEvent {
        parse_combat_line(msg).expect("combat line")
    }

    fn hit_of(e: LogEvent) -> (String, String, u64, String, String, String, bool) {
        match e {
            LogEvent::CombatHit {
                attacker,
                target,
                amount,
                ability,
                kind,
                outcome,
                incoming,
            } => (attacker, target, amount, ability, kind, outcome, incoming),
            other => panic!("expected CombatHit, got {other:?}"),
        }
    }

    #[test]
    fn melee_you_slash() {
        let (atk, tgt, amt, ability, kind, outcome, incoming) = hit_of(combat(
            "You slash a dar ghoul knight for 28 points of damage.",
        ));
        assert_eq!(atk, "You");
        assert_eq!(tgt, "a dar ghoul knight");
        assert_eq!(amt, 28);
        assert_eq!(ability, "slash");
        assert_eq!(kind, "melee");
        assert_eq!(outcome, "hit");
        assert!(!incoming);
    }

    #[test]
    fn melee_incoming() {
        let (atk, tgt, amt, _, _, _, incoming) = hit_of(combat(
            "A wan ghoul knight slashes YOU for 31 points of damage.",
        ));
        assert_eq!(atk, "A wan ghoul knight");
        assert_eq!(tgt, "YOU");
        assert_eq!(amt, 31);
        assert!(incoming);
    }

    #[test]
    fn frenzy_on() {
        let (_, _, amt, ability, _, _, _) = hit_of(combat(
            "You frenzy on a dar ghoul knight for 43 points of damage.",
        ));
        assert_eq!(amt, 43);
        assert_eq!(ability, "frenzy");
    }

    #[test]
    fn spell_by_name() {
        let (_, _, amt, ability, kind, _, _) = hit_of(combat(
            "You hit a dar ghoul knight for 123 points of magic damage by Smiting Strike.",
        ));
        assert_eq!(amt, 123);
        assert_eq!(ability, "Smiting Strike");
        assert_eq!(kind, "spell");
    }

    #[test]
    fn groupmate_spell() {
        let (atk, _, amt, ability, _, _, _) = hit_of(combat(
            "Vebn hit a dar ghoul knight for 90 points of fire damage by Flame Shock.",
        ));
        assert_eq!(atk, "Vebn");
        assert_eq!(amt, 90);
        assert_eq!(ability, "Flame Shock");
    }

    #[test]
    fn your_dot() {
        let (atk, tgt, amt, ability, kind, _, _) = hit_of(combat(
            "Hoptor Thaggelum has taken 213 damage from your Drifting Death.",
        ));
        assert_eq!(atk, "You");
        assert_eq!(tgt, "Hoptor Thaggelum");
        assert_eq!(amt, 213);
        assert_eq!(ability, "Drifting Death");
        assert_eq!(kind, "dot");
    }

    #[test]
    fn damage_shield_yours() {
        let (atk, tgt, amt, ability, kind, _, incoming) = hit_of(combat(
            "a rock golem is pierced by YOUR thorns for 24 points of non-melee damage.",
        ));
        assert_eq!(atk, "You");
        assert_eq!(tgt, "a rock golem");
        assert_eq!(amt, 24);
        assert!(ability.contains("thorns"));
        assert_eq!(kind, "ds");
        assert!(!incoming);
    }

    #[test]
    fn damage_shield_incoming() {
        let (_, tgt, amt, _, _, _, incoming) = hit_of(combat(
            "YOU are pierced by a vampire bat's thorns for 14 points of non-melee damage!",
        ));
        assert_eq!(tgt, "YOU");
        assert_eq!(amt, 14);
        assert!(incoming);
    }

    #[test]
    fn miss_and_dodge() {
        let (_, _, amt, ability, _, outcome, _) =
            hit_of(combat("You try to slash a gnoll, but miss!"));
        assert_eq!(amt, 0);
        assert_eq!(ability, "slash");
        assert_eq!(outcome, "miss");

        let (_, tgt, _, _, _, outcome, incoming) =
            hit_of(combat("A gnoll tries to slash YOU, but YOU dodge!"));
        assert_eq!(tgt, "YOU");
        assert_eq!(outcome, "dodge");
        assert!(incoming);
    }

    #[test]
    fn resist_your_spell() {
        let (atk, tgt, amt, ability, _, outcome, _) =
            hit_of(combat("A wan ghoul knight resisted your Dismiss Undead!"));
        assert_eq!(atk, "You");
        assert_eq!(tgt, "A wan ghoul knight");
        assert_eq!(amt, 0);
        assert_eq!(ability, "Dismiss Undead");
        assert_eq!(outcome, "resist");
    }

    #[test]
    fn heal_you() {
        let (atk, tgt, amt, _, kind, _, incoming) =
            hit_of(combat("You have been healed for 200 points."));
        assert_eq!(atk, "You");
        assert_eq!(tgt, "You");
        assert_eq!(amt, 200);
        assert_eq!(kind, "heal");
        assert!(incoming);
    }

    #[test]
    fn heal_other_by_you() {
        let (atk, tgt, amt, ability, kind, _, incoming) = hit_of(combat(
            "Vebn has been healed for 150 hit points by your Complete Heal.",
        ));
        assert_eq!(atk, "You");
        assert_eq!(tgt, "Vebn");
        assert_eq!(amt, 150);
        assert_eq!(ability, "Complete Heal");
        assert_eq!(kind, "heal");
        assert!(!incoming);
    }

    #[test]
    fn ignores_buff_land() {
        assert!(parse_combat_line("A cool breeze slips through your mind.").is_none());
        assert!(parse_combat_line("You begin casting Mesmerize.").is_none());
    }
}
