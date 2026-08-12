use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogEvent {
    /// Raw spell text from the cast line (may include a trailing Roman tier).
    /// The engine resolves base name + tier against the spell DB.
    BeginCast { spell: String },
    Interrupted,
    Fizzle,
    LandOther { target: String, message: String },
    LandYou { message: String },
    WearOff { message: String },
    MezBreak { target: String, breaker: String },
    Death { target: String },
    ZoneChange { zone: String },
    /// `You have gained a level! Welcome to level N!`
    LevelUp { level: u32 },
    Other,
}

fn re_begin_cast() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^You begin casting (.+)\.?$").unwrap())
}

fn re_interrupted() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^Your spell is interrupted\.?$|^Your casting has been interrupted\.?$")
            .unwrap()
    })
}

fn re_fizzle() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^Your spell fizzles\.?$").unwrap())
}

fn re_zone() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^You have entered (.+)\.?$").unwrap())
}

/// Self-kill: `You have slain a frenzied ghoul!` (EQL primary death line).
fn re_you_slain() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^You have slain (.+?)!?\s*$").unwrap())
}

/// Other-kill / NPC death: `A zol ghoul knight has been slain by Vebn!`
fn re_death() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(.+?) (?:has been slain by .+!|died\.?|has died\.?)$").unwrap()
    })
}

fn re_mez_break() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(.+?) has been awakened by (.+)\.?$").unwrap()
    })
}

fn re_level_up() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^You have gained a level! Welcome to level (\d+)!?\s*$").unwrap()
    })
}

/// Heuristic: self wear-off / expire chat lines (checked before LandYou).
///
/// Avoid bare `"fades"` matching **land** lines such as Shade/Shadow/Umbra
/// (`Your image fades`, `X's image fades around the edges`).
fn looks_like_wear_off(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    // Land lines for Shade / Shadow / Umbra / Invis (contain "fades" but are land).
    if lower.contains("image fades") {
        return false;
    }
    lower.starts_with("you are no longer")
        || lower.starts_with("your feet come free")
        || lower.contains("fades")
        || lower.contains(" fade")
        || lower.ends_with(" fade")
        || lower.ends_with(" fade.")
        || lower.contains("die down")
        || lower.contains("die away")
        || lower.contains("feel less serene")
        || lower.contains("feel confident again")
        || lower.contains("returns to normal")
        || lower.contains("has worn off")
        || lower.contains("worn off")
        || lower.contains("subsides")
        || lower.contains("has run its course")
        || lower.contains("are extinguished")
        || lower.contains("drifts away")
        || lower.contains("come free")
        || lower.contains("come into focus")
        || lower.contains("feel less ")
        || lower.contains("no longer")
        || lower.contains("leaves you")
        || lower.contains("departs")
        || lower.contains("dissipates")
        || lower.contains("melts away")
        || lower.contains("wears off")
}

/// Strip EQ timestamp prefix: `[Day Mon DD HH:MM:SS YYYY] message`
pub fn strip_timestamp(line: &str) -> &str {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('[') {
        if let Some(idx) = rest.find(']') {
            return rest[idx + 1..].trim();
        }
    }
    line
}

pub fn parse_line(line: &str) -> LogEvent {
    let msg = strip_timestamp(line);
    if msg.is_empty() {
        return LogEvent::Other;
    }

    if let Some(caps) = re_begin_cast().captures(msg) {
        let spell = caps[1].trim().trim_end_matches('.').to_string();
        return LogEvent::BeginCast { spell };
    }
    if re_interrupted().is_match(msg) {
        return LogEvent::Interrupted;
    }
    if re_fizzle().is_match(msg) {
        return LogEvent::Fizzle;
    }
    if let Some(caps) = re_zone().captures(msg) {
        return LogEvent::ZoneChange {
            zone: caps[1].trim().trim_end_matches('.').to_string(),
        };
    }
    if let Some(caps) = re_level_up().captures(msg) {
        if let Ok(level) = caps[1].parse::<u32>() {
            return LogEvent::LevelUp { level };
        }
    }
    if let Some(caps) = re_mez_break().captures(msg) {
        return LogEvent::MezBreak {
            target: caps[1].trim().to_string(),
            breaker: caps[2].trim().trim_end_matches('.').to_string(),
        };
    }
    // Prefer "You have slain X!" — the common EQL self-kill line — before the
    // other-kill / "died" patterns (which do not match "You have slain …").
    if let Some(caps) = re_you_slain().captures(msg) {
        return LogEvent::Death {
            target: caps[1].trim().trim_end_matches('!').trim().to_string(),
        };
    }
    if let Some(caps) = re_death().captures(msg) {
        return LogEvent::Death {
            target: caps[1].trim().to_string(),
        };
    }

    // Wear-off lines (self buffs / DoTs ending). Keep ahead of generic LandOther,
    // but after BeginCast / interrupt / zone / death. LandYou prefixes below run
    // after this — so exclude known land phrases inside looks_like_wear_off
    // (e.g. Shade: "Your image fades").
    if looks_like_wear_off(msg) {
        return LogEvent::WearOff {
            message: msg.to_string(),
        };
    }

    // Land on you: engine matches against spell land_you strings.
    // Include Clarity/Breeze family ("A cool/soft/light breeze…") and common
    // self-buff openings; do not use bare "A "/"An " — those match NPC land-other.
    if msg.starts_with("You are ")
        || msg.starts_with("You have been ")
        || msg.starts_with("You feel ")
        || msg.starts_with("A cool breeze")
        || msg.starts_with("A soft breeze")
        || msg.starts_with("A light breeze")
        || msg.starts_with("Part of your image")
        || msg.starts_with("Your feet become")
        || msg.starts_with("Your mind ")
        || msg.starts_with("Your body ")
        || msg.starts_with("Your skin ")
        || msg.starts_with("Your eyes ")
        || msg.starts_with("Your spirit ")
        || msg.starts_with("Your thoughts ")
        || msg.starts_with("Your image ")
        || msg.starts_with("Your muscles ")
        || msg.starts_with("Your hands ")
        || msg.starts_with("Your weapons ")
        || msg.starts_with("Your veins ")
        || msg.starts_with("Your blood ")
        || msg.starts_with("Your heart ")
        || msg.starts_with("Your wounds ")
        || msg.starts_with("Your sight ")
        || msg.starts_with("Your life ")
        || msg.starts_with("Your fingers ")
        || msg.starts_with("Your stomach ")
        // EQL Symbol line (Pinzarn / Ryltan / Transal / …)
        || msg.starts_with("The symbol of ")
        || msg.starts_with("A mystic symbol ")
    {
        return LogEvent::LandYou {
            message: msg.to_string(),
        };
    }

    // Generic land-other: keep full message; engine matches land_other substrings
    // and extracts the target as the text before the matched phrase.
    LogEvent::LandOther {
        target: String::new(),
        message: msg.to_string(),
    }
}

/// Normalize land phrases so wiki/"in" variants match EQL/"by" log text.
/// EQL logs insect DoTs as "engulfed by a swarm"; classic wiki often says "in".
fn normalize_land_text(s: &str) -> String {
    s.to_lowercase()
        .replace("engulfed in a swarm", "engulfed by a swarm")
}

/// If `message` contains `land_other` phrase, return the target name before it.
pub fn extract_target(message: &str, land_other: &str) -> Option<String> {
    let lower_msg = normalize_land_text(message);
    let lower_phrase = normalize_land_text(land_other);
    if let Some(idx) = lower_msg.find(&lower_phrase) {
        // Use the normalized index on the original message: lengths match for ASCII
        // land phrases (in↔by is same length), so byte offsets align.
        let target = message[..idx].trim().trim_end_matches('\'').trim();
        if !target.is_empty() {
            return Some(target.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_begin_cast() {
        let e = parse_line("[Wed Aug 5 23:00:00 2026] You begin casting Mesmerize.");
        assert_eq!(
            e,
            LogEvent::BeginCast {
                spell: "Mesmerize".into(),
            }
        );
    }

    #[test]
    fn parses_begin_cast_keeps_roman_suffix() {
        let e = parse_line("[Wed Aug 5 23:00:00 2026] You begin casting Spirit of Wolf V.");
        assert_eq!(
            e,
            LogEvent::BeginCast {
                spell: "Spirit of Wolf V".into(),
            }
        );
    }

    #[test]
    fn extracts_mez_target() {
        let t = extract_target("A gnoll has been mesmerized.", "has been mesmerized");
        assert_eq!(t.as_deref(), Some("A gnoll"));
    }

    #[test]
    fn extracts_swarm_dot_with_by_vs_in_mismatch() {
        // EQL: "engulfed by a swarm"; wiki/spells.json often: "engulfed in a swarm"
        let t = extract_target(
            "Hoptor Thaggelum is engulfed by a swarm.",
            "is engulfed in a swarm",
        );
        assert_eq!(t.as_deref(), Some("Hoptor Thaggelum"));
    }

    #[test]
    fn parses_interrupt() {
        assert_eq!(
            parse_line("[Wed Aug 5 23:00:00 2026] Your spell is interrupted."),
            LogEvent::Interrupted
        );
    }

    #[test]
    fn parses_clarity_self_land_as_land_you() {
        // Real EQL log line (Jungleberry); must not fall through to LandOther.
        let e = parse_line(
            "[Thu Aug 06 00:09:03 2026] A cool breeze slips through your mind.",
        );
        assert_eq!(
            e,
            LogEvent::LandYou {
                message: "A cool breeze slips through your mind.".into()
            }
        );
    }

    #[test]
    fn parses_symbol_of_pinzarn_self_land_as_land_you() {
        // Real EQL log line (Jungleberry); classic wiki says "A mystic symbol…".
        let e = parse_line(
            "[Fri Aug 07 16:41:05 2026] The symbol of Pinzarn flashes before your eyes.",
        );
        assert_eq!(
            e,
            LogEvent::LandYou {
                message: "The symbol of Pinzarn flashes before your eyes.".into()
            }
        );
    }

    #[test]
    fn parses_haste_wear_off_as_wear_off() {
        let e = parse_line("[Thu Aug 06 01:16:00 2026] Your speed returns to normal.");
        assert_eq!(
            e,
            LogEvent::WearOff {
                message: "Your speed returns to normal.".into()
            }
        );
    }

    #[test]
    fn parses_level_up() {
        let e = parse_line(
            "[Thu Aug 06 21:09:06 2026] You have gained a level! Welcome to level 43!",
        );
        assert_eq!(e, LogEvent::LevelUp { level: 43 });
    }

    #[test]
    fn parses_you_have_slain() {
        // Real EQL self-kill line (Jungleberry / Lower Guk).
        let e = parse_line("[Thu Aug 06 22:04:44 2026] You have slain a frenzied ghoul!");
        assert_eq!(
            e,
            LogEvent::Death {
                target: "a frenzied ghoul".into()
            }
        );
    }

    #[test]
    fn parses_has_been_slain_by() {
        let e = parse_line(
            "[Thu Aug 06 21:51:20 2026] A zol ghoul knight has been slain by Vebn!",
        );
        assert_eq!(
            e,
            LogEvent::Death {
                target: "A zol ghoul knight".into()
            }
        );
    }

    #[test]
    fn shade_land_is_land_you_not_wear_off() {
        // Contains "fades" but is Shade's land line — must not be WearOff.
        let e = parse_line("[Sun Aug 09 00:20:17 2026] Your image fades.");
        match e {
            LogEvent::LandYou { message } => {
                assert!(message.contains("Your image fades"));
            }
            other => panic!("expected LandYou, got {other:?}"),
        }
    }

    #[test]
    fn shade_ally_land_is_land_other_not_wear_off() {
        let e = parse_line("[Thu Aug 06 20:20:04 2026] Vebn's image fades around the edges.");
        match e {
            LogEvent::LandOther { message, .. } => {
                assert!(message.contains("image fades around the edges"));
            }
            other => panic!("expected LandOther, got {other:?}"),
        }
    }

    #[test]
    fn gift_of_magic_wear_off_still_wear_off() {
        let e = parse_line("[Sun Aug 09 00:00:00 2026] Your gift of magic fades.");
        match e {
            LogEvent::WearOff { .. } => {}
            other => panic!("expected WearOff, got {other:?}"),
        }
    }
}
