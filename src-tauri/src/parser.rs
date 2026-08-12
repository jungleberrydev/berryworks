use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// How an item left the corpse (EQL auto-loot / keep / depot / combine).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LootDisposition {
    /// `--You have looted …--` kept in inventory
    Kept,
    /// `… and sold it for …`
    Sold,
    /// `… and stored it in your currency|tradeskill depot|Dragon Hoard`
    Stored,
    /// `… to create a …` (combine / upgrade on loot)
    Combined,
}

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
    /// NPC death. `by_you` is true for `You have slain …!` or `… has been slain by You!`.
    /// `killer` is set for `… has been slain by NAME!` (including when NAME is You).
    Death {
        target: String,
        by_you: bool,
        killer: Option<String>,
    },
    ZoneChange { zone: String },
    /// `You have gained a level! Welcome to level N!`
    LevelUp { level: u32 },
    /// Item looted from a named corpse (mob is always present in EQL).
    LootItem {
        item: String,
        quantity: u32,
        mob: String,
        disposition: LootDisposition,
    },
    /// `You receive … from the corpse.` (no mob name — correlate via recent kill).
    CorpseCoin { copper: u64 },
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

/// Other-kill: `A zol ghoul knight has been slain by Vebn!`
fn re_slain_by() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(.+?) has been slain by (.+?)!\s*$").unwrap())
}

/// NPC death without a killer: `A froglok died.` / `A froglok has died.`
fn re_died() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(.+?) (?:died\.?|has died\.?)$").unwrap())
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

/// Kept: `--You have looted a ITEM from MOB's corpse.--`
fn re_loot_kept() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^--You have looted (?:a |an )?(.+?) from (.+?)'s corpse\.--\s*$",
        )
        .unwrap()
    })
}

/// Auto-sold / stored / combined loot from a corpse.
/// Captures: optional qty, item, mob, disposition tail.
fn re_loot_action() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^You looted (?:(\d+) )?(?:a |an )?(.+?) from (.+?)'s corpse (and sold it for .+|and stored it in your .+|to create .+)\.?\s*$",
        )
        .unwrap()
    })
}

/// Coin only: `You receive 5 silver and 3 copper from the corpse.`
fn re_corpse_coin() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^You receive (.+?) from the corpse\.?\s*$").unwrap()
    })
}

/// Parse EQ coin phrase into total copper (1p=1000, 1g=100, 1s=10, 1c=1).
pub fn parse_coin_to_copper(text: &str) -> Option<u64> {
    let lower = text.to_lowercase();
    if lower.trim().is_empty() || lower.contains("free") {
        return Some(0);
    }
    let re = Regex::new(
        r"(?i)(\d+)\s*(platinum|gold|silver|copper|platinums|golds|silvers|coppers|pp|gp|sp|cp)\b",
    )
    .ok()?;
    let mut total = 0u64;
    let mut matched = false;
    for caps in re.captures_iter(&lower) {
        matched = true;
        let n: u64 = caps[1].parse().ok()?;
        let unit = &caps[2];
        let mult = if unit.starts_with('p') {
            1000
        } else if unit.starts_with('g') {
            100
        } else if unit.starts_with('s') {
            10
        } else {
            1
        };
        total = total.saturating_add(n.saturating_mul(mult));
    }
    if matched {
        Some(total)
    } else {
        None
    }
}

fn loot_disposition_from_tail(tail: &str) -> LootDisposition {
    let lower = tail.to_lowercase();
    if lower.starts_with("and sold") {
        LootDisposition::Sold
    } else if lower.starts_with("and stored") {
        LootDisposition::Stored
    } else {
        LootDisposition::Combined
    }
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
            by_you: true,
            killer: None,
        };
    }
    if let Some(caps) = re_slain_by().captures(msg) {
        let target = caps[1].trim().to_string();
        let killer = caps[2].trim().trim_end_matches('!').trim().to_string();
        let by_you = killer.eq_ignore_ascii_case("You");
        return LogEvent::Death {
            target,
            by_you,
            killer: Some(killer),
        };
    }
    if let Some(caps) = re_died().captures(msg) {
        return LogEvent::Death {
            target: caps[1].trim().to_string(),
            by_you: false,
            killer: None,
        };
    }

    // Loot / corpse coin (EQL includes mob name on item lines).
    if let Some(caps) = re_loot_kept().captures(msg) {
        return LogEvent::LootItem {
            item: caps[1].trim().to_string(),
            quantity: 1,
            mob: caps[2].trim().to_string(),
            disposition: LootDisposition::Kept,
        };
    }
    if let Some(caps) = re_loot_action().captures(msg) {
        let quantity = caps
            .get(1)
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);
        return LogEvent::LootItem {
            item: caps[2].trim().to_string(),
            quantity,
            mob: caps[3].trim().to_string(),
            disposition: loot_disposition_from_tail(&caps[4]),
        };
    }
    if let Some(caps) = re_corpse_coin().captures(msg) {
        if let Some(copper) = parse_coin_to_copper(&caps[1]) {
            return LogEvent::CorpseCoin { copper };
        }
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
                target: "a frenzied ghoul".into(),
                by_you: true,
                killer: None,
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
                target: "A zol ghoul knight".into(),
                by_you: false,
                killer: Some("Vebn".into()),
            }
        );
    }

    #[test]
    fn parses_has_been_slain_by_you() {
        let e = parse_line(
            "[Thu Aug 06 21:51:20 2026] A zol ghoul knight has been slain by You!",
        );
        assert_eq!(
            e,
            LogEvent::Death {
                target: "A zol ghoul knight".into(),
                by_you: true,
                killer: Some("You".into()),
            }
        );
    }

    #[test]
    fn parses_loot_kept() {
        let e = parse_line(
            "[Tue Aug 11 23:10:48 2026] --You have looted a Drop of Crystallized Flame +4 from Unbound Flame's corpse.--",
        );
        assert_eq!(
            e,
            LogEvent::LootItem {
                item: "Drop of Crystallized Flame +4".into(),
                quantity: 1,
                mob: "Unbound Flame".into(),
                disposition: LootDisposition::Kept,
            }
        );
    }

    #[test]
    fn parses_loot_sold_with_qty() {
        let e = parse_line(
            "[Thu Jul 16 21:20:31 2026] You looted 2 Crystallized Sulfur from an ire ghast's corpse and sold it for 2 gold, 3 silver and 6 copper.",
        );
        assert_eq!(
            e,
            LogEvent::LootItem {
                item: "Crystallized Sulfur".into(),
                quantity: 2,
                mob: "an ire ghast".into(),
                disposition: LootDisposition::Sold,
            }
        );
    }

    #[test]
    fn parses_loot_stored_and_combined() {
        let stored = parse_line(
            "[Thu Jul 16 21:07:58 2026] You looted a Mote of Infinitesimal Potential from a spite golem's corpse and stored it in your currency",
        );
        assert_eq!(
            stored,
            LogEvent::LootItem {
                item: "Mote of Infinitesimal Potential".into(),
                quantity: 1,
                mob: "a spite golem".into(),
                disposition: LootDisposition::Stored,
            }
        );
        let combined = parse_line(
            "[Thu Jul 16 21:12:27 2026] You looted a Valorium Vambraces from an ire ghast's corpse to create a Valorium Vambraces +1",
        );
        assert_eq!(
            combined,
            LogEvent::LootItem {
                item: "Valorium Vambraces".into(),
                quantity: 1,
                mob: "an ire ghast".into(),
                disposition: LootDisposition::Combined,
            }
        );
    }

    #[test]
    fn parses_corpse_coin() {
        let e = parse_line(
            "[Tue Aug 11 23:08:49 2026] You receive 2 silver and 8 copper from the corpse.",
        );
        assert_eq!(e, LogEvent::CorpseCoin { copper: 28 });
    }

    #[test]
    fn ignores_vendor_coin_as_corpse() {
        let e = parse_line(
            "[Tue Aug 11 22:57:09 2026] You received 1 platinum, 4 gold, 2 silver and 9 copper from that item.",
        );
        assert!(matches!(e, LogEvent::LandOther { .. } | LogEvent::Other));
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
