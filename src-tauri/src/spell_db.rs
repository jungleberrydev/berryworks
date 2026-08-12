use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellClassLevel {
    pub class: String,
    pub level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellDef {
    pub name: String,
    pub category: String,
    pub duration_formula: String,
    pub base_ticks: u32,
    pub max_ticks: u32,
    pub tier_duration_pct: f64,
    pub land_other: String,
    #[serde(default)]
    pub land_you: String,
    #[serde(default)]
    pub wear_off_you: String,
    #[serde(default)]
    pub watched_by_default: bool,
    /// Class/level pairs from the wiki; a spell may appear under multiple classes.
    #[serde(default)]
    pub classes: Vec<SpellClassLevel>,
    /// Wiki / client spell icon id (numeric or letter), used for UI icons.
    #[serde(default)]
    pub spellicon: String,
}

/// Overlay visual preferences (colors, opacity, row size).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayAppearance {
    pub text_color: String,
    pub panel_color: String,
    pub buff_color: String,
    pub debuff_color: String,
    pub dot_color: String,
    /// Panel background opacity 0.0–1.0 (0 = fully transparent panel)
    pub panel_opacity: f64,
    /// Timer bar fill opacity 0.0–1.0
    pub bar_opacity: f64,
    /// "minimal" | "small" | "normal" | "large"
    pub timer_size: String,
    /// Preset theme id (user can still tweak colors after selecting).
    #[serde(default = "default_theme_id")]
    pub theme: String,
    /// Overlay UI font family (Windows system fonts; e.g. "Segoe UI").
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// Show spell icons on overlay timer rows.
    #[serde(default = "default_show_icons")]
    pub show_icons: bool,
    /// Right-click an unlocked overlay timer row to dismiss it.
    /// No effect while the overlay is locked (click-through).
    #[serde(default = "default_right_click_dismiss")]
    pub right_click_dismiss: bool,
    /// Show a muted "Recently wore off" section for timers that ended
    /// or were cleared within `recently_wore_off_secs`.
    /// Renew-relevant self/ally buffs (incl. Chloroplast/regen) — not blossom/celestial
    /// heal HoTs, invis, roots, or enemy debuff/DoT/lull.
    #[serde(default = "default_show_recently_wore_off")]
    pub show_recently_wore_off: bool,
    /// How long recently-wore-off rows stay visible (seconds). Clamped 15..=300.
    #[serde(default = "default_recently_wore_off_secs")]
    pub recently_wore_off_secs: u64,
    /// When true, enemy debuff/DoT timers show in a second always-on-top window
    /// (`overlay-enemies`); the main overlay keeps self + buff/ally timers.
    #[serde(default = "default_separate_enemy_window")]
    pub separate_enemy_window: bool,
    /// When true, the main/friendly overlay only shows timers on You
    /// (and `AppConfig.my_pet_name` when set). Enemies overlay is unchanged.
    #[serde(default = "default_self_buffs_only")]
    pub self_buffs_only: bool,
    /// When true, hide timers on other pets (`{Owner} pet` / known type names)
    /// on the main/friendly overlay. Your pet (`my_pet_name`) and non-pet allies stay.
    #[serde(default = "default_hide_other_pets")]
    pub hide_other_pets: bool,
    /// Speak spell name when a timer wears off or is dismissed (main overlay TTS).
    #[serde(default = "default_voice_announcements")]
    pub voice_announcements: bool,
    /// Web Speech voiceURI for announcements; empty = system default.
    #[serde(default = "default_voice_uri")]
    pub voice_uri: String,
    /// TTS volume 0.0–1.0 (SpeechSynthesisUtterance.volume).
    #[serde(default = "default_voice_volume")]
    pub voice_volume: f64,
    /// Flash (urgent pulse) when a timer has ≤ `expiry_warn_secs` remaining.
    #[serde(default = "default_flash_expiry_warn")]
    pub flash_expiry_warn: bool,
    /// Speak spell name when a timer crosses into the expiry-warn window (main overlay).
    #[serde(default = "default_verbal_expiry_warn")]
    pub verbal_expiry_warn: bool,
    /// Shared lead time (seconds) for flash + verbal pre-expiry alerts. Clamped 1..=30.
    #[serde(default = "default_expiry_warn_secs")]
    pub expiry_warn_secs: u64,
    /// Show the independent respawn overlay window.
    #[serde(default = "default_show_respawn_window")]
    pub show_respawn_window: bool,
    /// Start a zone-default respawn timer on every kill (rares still use overrides).
    #[serde(default = "default_track_all_kills")]
    pub track_all_kills: bool,
    /// Deprecated: native DWM border/shadow is tied to overlay lock state
    /// (on when unlocked for drag hit-testing; off when locked). Kept for
    /// config backwards-compat only — ignored at runtime.
    #[serde(default = "default_show_window_border")]
    pub show_window_border: bool,
}

fn default_theme_id() -> String {
    "berry".into()
}

fn default_font_family() -> String {
    "Segoe UI".into()
}

fn default_show_icons() -> bool {
    true
}

fn default_right_click_dismiss() -> bool {
    true
}

fn default_show_recently_wore_off() -> bool {
    true
}

fn default_recently_wore_off_secs() -> u64 {
    300
}

fn default_separate_enemy_window() -> bool {
    false
}

fn default_self_buffs_only() -> bool {
    false
}

fn default_hide_other_pets() -> bool {
    false
}

fn default_voice_announcements() -> bool {
    true
}

fn default_voice_uri() -> String {
    String::new()
}

fn default_voice_volume() -> f64 {
    1.0
}

fn default_flash_expiry_warn() -> bool {
    true
}

fn default_verbal_expiry_warn() -> bool {
    false
}

fn default_expiry_warn_secs() -> u64 {
    30
}

/// Recently-wore-off retention window bounds (seconds).
pub const RECENTLY_WORE_OFF_SECS_MIN: u64 = 15;
pub const RECENTLY_WORE_OFF_SECS_MAX: u64 = 300;

/// Pre-expiry flash/verbal lead-time bounds (seconds).
pub const EXPIRY_WARN_SECS_MIN: u64 = 1;
pub const EXPIRY_WARN_SECS_MAX: u64 = 30;

impl OverlayAppearance {
    /// Clamped retention for recently-wore-off rows (15..=300 seconds).
    pub fn recently_wore_off_secs_clamped(&self) -> u64 {
        self.recently_wore_off_secs
            .clamp(RECENTLY_WORE_OFF_SECS_MIN, RECENTLY_WORE_OFF_SECS_MAX)
    }

    /// Clamped TTS volume (0.0..=1.0).
    pub fn voice_volume_clamped(&self) -> f64 {
        if !self.voice_volume.is_finite() {
            return 1.0;
        }
        self.voice_volume.clamp(0.0, 1.0)
    }

    /// Clamped pre-expiry warn lead time (1..=30 seconds).
    pub fn expiry_warn_secs_clamped(&self) -> u64 {
        self.expiry_warn_secs
            .clamp(EXPIRY_WARN_SECS_MIN, EXPIRY_WARN_SECS_MAX)
    }
}

fn default_show_respawn_window() -> bool {
    true
}

fn default_track_all_kills() -> bool {
    true
}

fn default_show_window_border() -> bool {
    false
}

impl Default for OverlayAppearance {
    fn default() -> Self {
        Self {
            text_color: "#f6ebf1".into(),
            panel_color: "#160e14".into(),
            buff_color: "#7eb8a2".into(),
            debuff_color: "#c45c8a".into(),
            dot_color: "#d4a05a".into(),
            panel_opacity: 0.82,
            bar_opacity: 1.0,
            timer_size: "normal".into(),
            theme: default_theme_id(),
            font_family: default_font_family(),
            show_icons: true,
            right_click_dismiss: true,
            show_recently_wore_off: true,
            recently_wore_off_secs: default_recently_wore_off_secs(),
            separate_enemy_window: false,
            self_buffs_only: false,
            hide_other_pets: false,
            voice_announcements: true,
            voice_uri: default_voice_uri(),
            voice_volume: default_voice_volume(),
            flash_expiry_warn: default_flash_expiry_warn(),
            verbal_expiry_warn: default_verbal_expiry_warn(),
            expiry_warn_secs: default_expiry_warn_secs(),
            show_respawn_window: true,
            track_all_kills: true,
            show_window_border: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub log_path: String,
    pub character_level: u32,
    /// Spell Casting Reinforcement AA rank (0 = none, 1–4 = +5/15/30/50%).
    /// Extends beneficial spells you cast; invulnerability and combat abilities
    /// are exempt.
    #[serde(default)]
    pub spell_casting_reinforcement: u32,
    /// Exact pet target string as it appears in land/combat logs
    /// (e.g. `Gastik` or `Jungleberry pet`). See `data/pets.json`.
    #[serde(default)]
    pub my_pet_name: String,
    /// spell name -> tier 0-10
    pub spell_tiers: HashMap<String, u32>,
    /// spell name -> watched
    pub watched: HashMap<String, bool>,
    /// rare camp id -> watched
    #[serde(default)]
    pub watched_rares: HashMap<String, bool>,
    /// rare id or zone id -> custom respawn seconds override
    #[serde(default)]
    pub camp_overrides: HashMap<String, u64>,
    /// Last known / manually selected camping zone (display name).
    #[serde(default)]
    pub respawn_zone: String,
    /// When true, parse loot/kill lines into local loot.json drop stats.
    #[serde(default = "default_loot_tracking")]
    pub loot_tracking: bool,
    /// Opt-in upload of aggregates to Norrath Roster.
    #[serde(default)]
    pub loot_sync_enabled: bool,
    /// Base site URL (no trailing slash), e.g. https://norrathroster.com
    #[serde(default = "default_loot_sync_url")]
    pub loot_sync_url: String,
    /// Legacy/ops shared ingest key (optional; prefer Discord upload token).
    #[serde(default)]
    pub loot_sync_key: String,
    /// Discord-issued Berryworks loot upload token (Bearer).
    #[serde(default)]
    pub loot_upload_token: String,
    /// Discord username from last successful Berryworks login.
    #[serde(default)]
    pub loot_discord_username: String,
    /// Discord global display name when available.
    #[serde(default)]
    pub loot_discord_global_name: String,
    /// Discord snowflake for the signed-in loot uploader.
    #[serde(default)]
    pub loot_discord_user_id: String,
    /// Stable anonymous contributor UUID for ops-key uploads.
    #[serde(default)]
    pub loot_contributor_id: String,
    pub overlay_locked: bool,
    #[serde(default)]
    pub overlay: OverlayAppearance,
}

fn default_loot_tracking() -> bool {
    true
}

fn default_loot_sync_url() -> String {
    "https://norrathroster.com".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            log_path: String::new(),
            character_level: 1,
            spell_casting_reinforcement: 0,
            my_pet_name: String::new(),
            spell_tiers: HashMap::new(),
            watched: HashMap::new(),
            watched_rares: HashMap::new(),
            camp_overrides: HashMap::new(),
            respawn_zone: String::new(),
            loot_tracking: true,
            loot_sync_enabled: false,
            loot_sync_url: default_loot_sync_url(),
            loot_sync_key: String::new(),
            loot_upload_token: String::new(),
            loot_discord_username: String::new(),
            loot_discord_global_name: String::new(),
            loot_discord_user_id: String::new(),
            loot_contributor_id: String::new(),
            overlay_locked: false,
            overlay: OverlayAppearance::default(),
        }
    }
}

/// Highest Spell Casting Reinforcement rank (Mastery / +50%).
pub const SPELL_CASTING_REINFORCEMENT_MAX_RANK: u32 = 4;

/// Clamp overlay fields that have valid ranges (called on load/save).
pub fn normalize_config(config: &mut AppConfig) {
    config.spell_casting_reinforcement = config
        .spell_casting_reinforcement
        .min(SPELL_CASTING_REINFORCEMENT_MAX_RANK);
    config.my_pet_name = config.my_pet_name.trim().to_string();
    config.overlay.recently_wore_off_secs = config.overlay.recently_wore_off_secs_clamped();
    config.overlay.voice_volume = config.overlay.voice_volume_clamped();
    config.overlay.expiry_warn_secs = config.overlay.expiry_warn_secs_clamped();
    // Always use production sync URL (field kept for config load compat; not user-editable).
    config.loot_sync_url = default_loot_sync_url();
    config.loot_sync_key = config.loot_sync_key.trim().to_string();
    config.loot_upload_token = config.loot_upload_token.trim().to_string();
    config.loot_discord_username = config.loot_discord_username.trim().to_string();
    config.loot_discord_global_name = config.loot_discord_global_name.trim().to_string();
    config.loot_discord_user_id = config.loot_discord_user_id.trim().to_string();
    if config.loot_contributor_id.trim().is_empty() {
        config.loot_contributor_id = uuid::Uuid::new_v4().to_string();
    } else {
        config.loot_contributor_id = config.loot_contributor_id.trim().to_string();
    }
}

pub fn load_spells() -> Result<Vec<SpellDef>, String> {
    let candidates = [
        PathBuf::from("data/spells.json"),
        PathBuf::from("../data/spells.json"),
        resource_spells_path(),
    ];
    for path in candidates {
        if path.exists() {
            let raw = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            return serde_json::from_str(&raw).map_err(|e| format!("Invalid spells.json: {e}"));
        }
    }
    // Embedded fallback
    let raw = include_str!("../../data/spells.json");
    serde_json::from_str(raw).map_err(|e| format!("Invalid embedded spells.json: {e}"))
}

fn resource_spells_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("resources").join("spells.json");
        }
    }
    PathBuf::from("resources/spells.json")
}

pub fn config_path() -> PathBuf {
    dirs_config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("berry-timers")
        .join("config.json")
}

fn dirs_config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
        })
}

pub fn load_config(spells: &[SpellDef]) -> AppConfig {
    let path = config_path();
    let mut config = if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        AppConfig::default()
    };
    normalize_config(&mut config);

    for spell in spells {
        config
            .spell_tiers
            .entry(spell.name.clone())
            .or_insert(0);
        config
            .watched
            .entry(spell.name.clone())
            .or_insert(spell.watched_by_default);
    }
    config
}

/// Seed watched_rares defaults from camps.json rares.
pub fn seed_watched_rares(config: &mut AppConfig, camps: &crate::spawn_db::CampsFile) {
    for zone in &camps.zones {
        for rare in &zone.rares {
            config
                .watched_rares
                .entry(rare.id.clone())
                .or_insert(rare.watched_by_default);
        }
    }
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let mut normalized = config.clone();
    normalize_config(&mut normalized);
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())
}

/// Classic EQ `buffdurationformula` IDs (EQL `spells_us.txt` field 11) → tick count.
/// Caps are applied by the caller via `max_ticks` (client field 12).
///
/// | ID | formula string | ticks |
/// |----|----------------|-------|
/// | 1 | `level_div_2` | level/2 |
/// | 2 | `level_div_2_plus_5` | level/2+5 (or 6 if level≤3) |
/// | 3 | `level_x30` | level×30 |
/// | 4 | `fixed_50` | 50 |
/// | 5 | `fixed_2` | 2 |
/// | 6 | `level_div_2_plus_2` | level/2+2 |
/// | 7 | `level` | level |
/// | 8 | `level_plus_10` | level+10 |
/// | 9 | `level_x2_plus_10` | level×2+10 |
/// | 10 | `level_x3_plus_10` | level×3+10 |
/// | 11 | `level_plus_3_x30` | 30×(level+3) |
/// | 12 | `level_div_4` | level/4 (or 1 if level≤7) |
/// | 13 | `level_x4_plus_10` | level×4+10 |
/// | 14 | `level_plus_2_x5` | 5×(level+2) |
/// | 15 | `level_plus_10_x10` | 10×(level+10) |
/// | 50 | `f50` | permanent (5 days / no timer) |
/// | 51 | `permanent` | permanent |
pub fn ticks_from_formula(formula: &str, level: u32, base_ticks: u32) -> u32 {
    match formula {
        "level_div_2" => level / 2,
        "level_div_2_plus_5" => {
            if level > 3 {
                level / 2 + 5
            } else {
                6
            }
        }
        "level_div_2_plus_2" => level / 2 + 2,
        "level_x30" => level.saturating_mul(30),
        "fixed_50" => 50,
        "fixed_2" => 2,
        "level" => level,
        "level_plus_10" => level.saturating_add(10),
        "level_x2_plus_10" => level.saturating_mul(2).saturating_add(10),
        "level_x3_plus_10" => level.saturating_mul(3).saturating_add(10),
        "level_plus_3_x30" => level.saturating_add(3).saturating_mul(30),
        "level_div_4" => {
            if level > 7 {
                level / 4
            } else {
                1
            }
        }
        "level_x4_plus_10" => level.saturating_mul(4).saturating_add(10),
        "level_plus_2_x5" => level.saturating_add(2).saturating_mul(5),
        "level_plus_10_x10" => level.saturating_add(10).saturating_mul(10),
        // Permanent / no countdown — callers should not start a timer.
        "f50" | "permanent" => 0,
        "fixed" | _ => base_ticks,
    }
}

/// Spell Casting Reinforcement rank → duration bonus percent.
/// Rank 0 = none; 1 = 5%; 2 = 15%; 3 = 30%; 4 = 50%.
pub fn spell_casting_reinforcement_pct(rank: u32) -> f64 {
    match rank.min(SPELL_CASTING_REINFORCEMENT_MAX_RANK) {
        1 => 5.0,
        2 => 15.0,
        3 => 30.0,
        4 => 50.0,
        _ => 0.0,
    }
}

/// Beneficial spells you cast, excluding invulnerability and combat abilities.
pub fn spell_eligible_for_reinforcement(spell: &SpellDef) -> bool {
    if is_invulnerability_spell(spell) || is_combat_ability_spell(spell) {
        return false;
    }
    let cat = spell.category.to_ascii_lowercase();
    cat == "buff" || cat == "lull"
}

fn is_invulnerability_spell(spell: &SpellDef) -> bool {
    const NAMES: &[&str] = &[
        "Divine Aura",
        "Divine Barrier",
        "Harmshield",
        "Quivering Veil of Xarn",
    ];
    if NAMES.iter().any(|n| spell.name.eq_ignore_ascii_case(n)) {
        return true;
    }
    let blob = format!(
        "{} {} {}",
        spell.land_you, spell.land_other, spell.wear_off_you
    )
    .to_ascii_lowercase();
    blob.contains("invulnerab")
}

fn is_combat_ability_spell(spell: &SpellDef) -> bool {
    let cat = spell.category.to_ascii_lowercase();
    cat == "combat" || cat == "combat_ability" || cat == "discipline"
}

/// Compute duration in seconds from spell definition, caster level, and tier.
pub fn duration_seconds(spell: &SpellDef, level: u32, tier: u32) -> u64 {
    duration_seconds_with_aa(spell, level, tier, 0)
}

/// Like [`duration_seconds`], plus Spell Casting Reinforcement when the spell
/// is eligible (beneficial, not invuln / combat ability). `scr_rank` 0–4.
pub fn duration_seconds_with_aa(spell: &SpellDef, level: u32, tier: u32, scr_rank: u32) -> u64 {
    let tier = tier.min(10);
    let mut ticks = ticks_from_formula(&spell.duration_formula, level, spell.base_ticks);

    if spell.max_ticks > 0 {
        ticks = ticks.min(spell.max_ticks);
    }
    if spell.base_ticks > 0 && spell.duration_formula == "fixed" {
        ticks = spell.base_ticks;
    }

    let bonus = 1.0 + (tier as f64) * (spell.tier_duration_pct / 100.0);
    let mut effective = (ticks as f64) * bonus;
    if spell_eligible_for_reinforcement(spell) {
        let scr = 1.0 + spell_casting_reinforcement_pct(scr_rank) / 100.0;
        if scr > 1.0 {
            effective *= scr;
        }
    }
    let effective = effective.round() as u32;
    let capped = if spell.max_ticks > 0 {
        // Tier can push past listed wiki max; allow rounded value but keep a soft ceiling
        // of max_ticks * 2.5 so rank 10 (~+100%) still works.
        effective.min(spell.max_ticks.saturating_mul(3))
    } else {
        effective
    };

    (capped as u64) * 6
}

pub fn find_spell_by_name<'a>(spells: &'a [SpellDef], name: &str) -> Option<&'a SpellDef> {
    resolve_cast_spell(spells, name).map(|(spell, _)| spell)
}

/// Resolve a cast-line spell name to a spell def and EQL tier (0–10).
///
/// Exact DB names win first so distinct spells like `"Clarity II"` are not
/// mistaken for `"Clarity"` at tier 2. Otherwise a trailing Roman numeral is
/// treated as the upgrade tier (`"Spirit of Wolf V"` → Spirit of Wolf, tier 5).
///
/// Wiki disambiguators like `"Shield of Thorns (Spell)"` are matched when the
/// cast line is bare `"Shield of Thorns"` (and the reverse).
pub fn resolve_cast_spell<'a>(
    spells: &'a [SpellDef],
    raw_name: &str,
) -> Option<(&'a SpellDef, u32)> {
    let raw = raw_name.trim().trim_end_matches('.');
    if raw.is_empty() {
        return None;
    }

    if let Some(spell) = find_spell_def_by_name(spells, raw) {
        return Some((spell, 0));
    }

    let (base, tier) = parse_spell_name_and_tier(raw);
    find_spell_def_by_name(spells, &base).map(|spell| (spell, tier))
}

/// Exact name, then wiki `" (Spell)"` disambiguation alias either direction.
fn find_spell_def_by_name<'a>(spells: &'a [SpellDef], name: &str) -> Option<&'a SpellDef> {
    if let Some(spell) = spells
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
    {
        return Some(spell);
    }
    let with_suffix = format!("{name} (Spell)");
    if let Some(spell) = spells
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(&with_suffix))
    {
        return Some(spell);
    }
    if let Some(base) = name
        .strip_suffix(" (Spell)")
        .or_else(|| name.strip_suffix(" (spell)"))
    {
        let base = base.trim();
        if !base.is_empty() {
            return spells
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(base));
        }
    }
    None
}

/// EQL Roman-numeral ranks on cast lines: no suffix = tier 0, I–X = tiers 1–10.
/// Longest match first so VIII wins over V / I, etc.
const ROMAN_TIERS: &[(&str, u32)] = &[
    ("VIII", 8),
    ("VII", 7),
    ("III", 3),
    ("II", 2),
    ("IX", 9),
    ("IV", 4),
    ("VI", 6),
    ("X", 10),
    ("V", 5),
    ("I", 1),
];

/// Parse base spell name and EQL tier from a cast/log spell string.
///
/// - `"Spirit of Wolf V"` → `("Spirit of Wolf", 5)`
/// - `"Mesmerize"` → `("Mesmerize", 0)`
/// - Live-style `"Foo Rk. II"` is stripped to `"Foo"` with tier 0 (Rk. is not EQL tier).
pub fn parse_spell_name_and_tier(name: &str) -> (String, u32) {
    let mut s = name.trim().trim_end_matches('.').to_string();

    for suffix in [" Rk. III", " Rk. II", " Rk. I", " Rk.III", " Rk.II", " Rk.I"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.trim_end().to_string();
            break;
        }
    }

    let upper = s.to_ascii_uppercase();
    for &(roman, tier) in ROMAN_TIERS {
        let suffix = format!(" {roman}");
        if upper.ends_with(&suffix) {
            let base_len = s.len().saturating_sub(suffix.len());
            let base = s[..base_len].trim_end().to_string();
            if !base.is_empty() {
                return (base, tier);
            }
        }
    }

    (s, 0)
}

/// Strip EQL Roman tiers and Live-style " Rk. II" suffixes for name matching.
pub fn strip_upgrade_suffix(name: &str) -> String {
    parse_spell_name_and_tier(name).0
}

pub fn is_watched(config: &AppConfig, spell_name: &str) -> bool {
    // Any explicit `true` among aliases wins so stale config keys like
    // `"Shield of Thorns (Spell)"` still enable `"Shield of Thorns"` after rename.
    let keys = spell_watch_keys(spell_name);
    if keys
        .iter()
        .any(|k| config.watched.get(k).copied() == Some(true))
    {
        return true;
    }
    if keys.iter().any(|k| config.watched.contains_key(k)) {
        return false;
    }
    false
}

/// Config lookup keys for a spell: exact, Roman/Rk-stripped, and `(Spell)` alias.
fn spell_watch_keys(spell_name: &str) -> Vec<String> {
    let cleaned = strip_upgrade_suffix(spell_name);
    let mut keys: Vec<String> = vec![spell_name.to_string()];
    if !cleaned.eq_ignore_ascii_case(spell_name) {
        keys.push(cleaned.clone());
    }
    for name in [spell_name, cleaned.as_str()] {
        if let Some(base) = name
            .strip_suffix(" (Spell)")
            .or_else(|| name.strip_suffix(" (spell)"))
        {
            let base = base.trim();
            if !base.is_empty() && !keys.iter().any(|k| k.eq_ignore_ascii_case(base)) {
                keys.push(base.to_string());
            }
        } else {
            let with_suffix = format!("{name} (Spell)");
            if !keys.iter().any(|k| k.eq_ignore_ascii_case(&with_suffix)) {
                keys.push(with_suffix);
            }
        }
    }
    keys
}

/// Legacy config lookup; cast-line Roman tiers are preferred by the engine.
#[allow(dead_code)]
pub fn spell_tier(config: &AppConfig, spell_name: &str) -> u32 {
    let cleaned = strip_upgrade_suffix(spell_name);
    config
        .spell_tiers
        .get(&cleaned)
        .or_else(|| config.spell_tiers.get(spell_name))
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mez() -> SpellDef {
        SpellDef {
            name: "Mesmerize".into(),
            category: "debuff".into(),
            duration_formula: "fixed".into(),
            base_ticks: 4,
            max_ticks: 4,
            tier_duration_pct: 10.0,
            land_other: "has been mesmerized".into(),
            land_you: String::new(),
            wear_off_you: String::new(),
            watched_by_default: true,
            classes: vec![],
            spellicon: String::new(),
        }
    }

    #[test]
    fn mez_tier_0_is_24_seconds() {
        assert_eq!(duration_seconds(&mez(), 50, 0), 24);
    }

    #[test]
    fn mez_tier_10_doubles() {
        // 4 * (1 + 1.0) = 8 ticks = 48s
        assert_eq!(duration_seconds(&mez(), 50, 10), 48);
    }

    #[test]
    fn strip_rk_suffix() {
        assert_eq!(strip_upgrade_suffix("Mesmerize Rk. II"), "Mesmerize");
    }

    #[test]
    fn roman_tier_mapping() {
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf"),
            ("Spirit of Wolf".into(), 0)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf I"),
            ("Spirit of Wolf".into(), 1)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf II"),
            ("Spirit of Wolf".into(), 2)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf III"),
            ("Spirit of Wolf".into(), 3)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf IV"),
            ("Spirit of Wolf".into(), 4)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf V"),
            ("Spirit of Wolf".into(), 5)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf VI"),
            ("Spirit of Wolf".into(), 6)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf VII"),
            ("Spirit of Wolf".into(), 7)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf VIII"),
            ("Spirit of Wolf".into(), 8)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf IX"),
            ("Spirit of Wolf".into(), 9)
        );
        assert_eq!(
            parse_spell_name_and_tier("Spirit of Wolf X"),
            ("Spirit of Wolf".into(), 10)
        );
    }

    #[test]
    fn duration_uses_tier_from_cast_name() {
        let (base, tier) = parse_spell_name_and_tier("Mesmerize V");
        assert_eq!(base, "Mesmerize");
        assert_eq!(tier, 5);
        // 4 ticks * (1 + 5*0.10) = 4 * 1.5 = 6 ticks = 36s
        assert_eq!(duration_seconds(&mez(), 50, tier), 36);
    }

    #[test]
    fn resolve_prefers_exact_roman_named_spell() {
        let spells = vec![
            SpellDef {
                name: "Clarity".into(),
                category: "buff".into(),
                duration_formula: "fixed".into(),
                base_ticks: 270,
                max_ticks: 270,
                tier_duration_pct: 10.0,
                land_other: "looks tranquil".into(),
                land_you: "A cool breeze slips through your mind".into(),
                wear_off_you: String::new(),
                watched_by_default: true,
                classes: vec![SpellClassLevel {
                    class: "Enchanter".into(),
                    level: 26,
                }],
                spellicon: String::new(),
            },
            SpellDef {
                name: "Clarity II".into(),
                category: "buff".into(),
                duration_formula: "fixed".into(),
                base_ticks: 350,
                max_ticks: 350,
                tier_duration_pct: 10.0,
                land_other: "looks very tranquil".into(),
                land_you: "A soft breeze slips through your mind".into(),
                wear_off_you: String::new(),
                watched_by_default: false,
                classes: vec![SpellClassLevel {
                    class: "Enchanter".into(),
                    level: 54,
                }],
                spellicon: String::new(),
            },
            SpellDef {
                name: "Spirit of Wolf".into(),
                category: "buff".into(),
                duration_formula: "fixed".into(),
                base_ticks: 360,
                max_ticks: 360,
                tier_duration_pct: 10.0,
                land_other: String::new(),
                land_you: String::new(),
                wear_off_you: String::new(),
                watched_by_default: true,
                classes: vec![],
                spellicon: String::new(),
            },
        ];

        let (spell, tier) = resolve_cast_spell(&spells, "Clarity II").unwrap();
        assert_eq!(spell.name, "Clarity II");
        assert_eq!(tier, 0);

        let (spell, tier) = resolve_cast_spell(&spells, "Spirit of Wolf V").unwrap();
        assert_eq!(spell.name, "Spirit of Wolf");
        assert_eq!(tier, 5);
    }

    #[test]
    fn spells_json_includes_classes() {
        let spells = load_spells().expect("spells");
        let clarity = spells.iter().find(|s| s.name == "Clarity").expect("Clarity");
        assert!(
            clarity
                .classes
                .iter()
                .any(|c| c.class == "Enchanter" && c.level == 26),
            "Clarity should list Enchanter 26, got {:?}",
            clarity.classes
        );
        let alacrity = spells
            .iter()
            .find(|s| s.name == "Alacrity")
            .expect("Alacrity");
        assert!(
            alacrity.classes.len() >= 2,
            "Alacrity should be multi-class, got {:?}",
            alacrity.classes
        );
    }

    fn haste(name: &str, formula: &str, max_ticks: u32) -> SpellDef {
        SpellDef {
            name: name.into(),
            category: "buff".into(),
            duration_formula: formula.into(),
            base_ticks: 0,
            max_ticks,
            tier_duration_pct: 10.0,
            land_other: "feels much faster".into(),
            land_you: "You feel much faster".into(),
            wear_off_you: "Your speed returns to normal".into(),
            watched_by_default: false,
            classes: vec![],
            spellicon: String::new(),
        }
    }

    #[test]
    fn celerity_level_scales_to_13m36_at_42() {
        // Classic formula 5: ticks = level×3+10, cap 160.
        // L42 → 136 ticks × 6s = 816s = 13:36 (wiki "16 Min" is the L50 cap).
        let spell = haste("Celerity", "level_x3_plus_10", 160);
        assert_eq!(duration_seconds(&spell, 39, 0), 762); // 127 ticks
        assert_eq!(duration_seconds(&spell, 42, 0), 816); // 136 ticks
        assert_eq!(duration_seconds(&spell, 50, 0), 960); // 160 ticks cap
        assert_eq!(duration_seconds(&spell, 60, 0), 960);
    }

    #[test]
    fn celerity_from_spells_json_at_level_42() {
        let spells = load_spells().expect("spells");
        let celerity = spells
            .iter()
            .find(|s| s.name == "Celerity")
            .expect("Celerity");
        assert_eq!(celerity.duration_formula, "level_x3_plus_10");
        assert_eq!(celerity.max_ticks, 160);
        assert_eq!(duration_seconds(celerity, 42, 0), 816);
        assert_eq!(duration_seconds(celerity, 50, 0), 960);
    }

    #[test]
    fn quickness_and_alacrity_use_level_x2_plus_10() {
        let q = haste("Quickness", "level_x2_plus_10", 110);
        assert_eq!(duration_seconds(&q, 16, 0), 252); // 42 ticks
        assert_eq!(duration_seconds(&q, 50, 0), 660); // 110 cap
        let a = haste("Alacrity", "level_x2_plus_10", 110);
        assert_eq!(duration_seconds(&a, 24, 0), 348); // 58 ticks
        assert_eq!(duration_seconds(&a, 42, 0), 564); // 94 ticks
    }

    #[test]
    fn strengthen_uses_level_plus_3_x30() {
        // Client formula 11: 30*(level+3), cap 270.
        // L1 → 120 ticks = 12:00 (wiki "27 Min" is the L6+ cap).
        let spell = haste("Strengthen", "level_plus_3_x30", 270);
        assert_eq!(duration_seconds(&spell, 1, 0), 720);
        assert_eq!(duration_seconds(&spell, 6, 0), 1620);
        assert_eq!(duration_seconds(&spell, 50, 0), 1620);
    }

    #[test]
    fn spirit_of_wolf_uses_level_x30() {
        // Client formula 3: level×30, cap 360.
        // L9 → 270 ticks = 27:00; L12+ → 36:00 cap.
        let spell = haste("Spirit of Wolf", "level_x30", 360);
        assert_eq!(duration_seconds(&spell, 9, 0), 1620);
        assert_eq!(duration_seconds(&spell, 12, 0), 2160);
        assert_eq!(duration_seconds(&spell, 50, 0), 2160);
    }

    #[test]
    fn snare_uses_level_div_2_plus_5() {
        // Client formula 2: level/2+5 (or 6 if level≤3), cap 39.
        let spell = haste("Snare", "level_div_2_plus_5", 39);
        assert_eq!(duration_seconds(&spell, 1, 0), 36); // 6 ticks
        assert_eq!(duration_seconds(&spell, 10, 0), 60); // 10 ticks
        assert_eq!(duration_seconds(&spell, 50, 0), 180); // 30 ticks
        assert_eq!(duration_seconds(&spell, 68, 0), 234); // 39 cap
    }

    #[test]
    fn high_value_batch_from_spells_json() {
        let spells = load_spells().expect("spells");
        let strengthen = spells.iter().find(|s| s.name == "Strengthen").unwrap();
        assert_eq!(strengthen.duration_formula, "level_plus_3_x30");
        assert_eq!(strengthen.max_ticks, 270);
        assert_eq!(duration_seconds(strengthen, 1, 0), 720);

        let sow = spells.iter().find(|s| s.name == "Spirit of Wolf").unwrap();
        assert_eq!(sow.duration_formula, "level_x30");
        assert_eq!(duration_seconds(sow, 9, 0), 1620);

        let ensnare = spells.iter().find(|s| s.name == "Ensnare").unwrap();
        assert_eq!(ensnare.duration_formula, "level_x2_plus_10");
        assert_eq!(ensnare.max_ticks, 140);
        assert_eq!(duration_seconds(ensnare, 26, 0), 372); // 62 ticks

        let regen = spells.iter().find(|s| s.name == "Regeneration").unwrap();
        assert_eq!(regen.duration_formula, "level_x3_plus_10");
        assert_eq!(regen.max_ticks, 205);
        assert_eq!(duration_seconds(regen, 23, 0), 474); // 79 ticks
    }

    #[test]
    fn chloroplast_matches_spells_us_and_jungleberry_l42() {
        // spells_us.txt: Chloroplast id 145, buffdurationformula=10, buffduration=205
        // Jungleberry L42 land 20:02:10 → wear 20:15:49 ≈ 819s (136 ticks × 6 = 816s).
        let spells = load_spells().expect("spells");
        let chloro = spells
            .iter()
            .find(|s| s.name == "Chloroplast")
            .expect("Chloroplast");
        assert_eq!(chloro.duration_formula, "level_x3_plus_10");
        assert_eq!(chloro.max_ticks, 205);
        assert_eq!(duration_seconds(chloro, 42, 0), 816); // 136 ticks
        assert_eq!(duration_seconds(chloro, 43, 0), 834); // 139 ticks
        assert_eq!(duration_seconds(chloro, 50, 0), 960); // 160 ticks
        assert_eq!(duration_seconds(chloro, 65, 0), 1230); // 205 cap

        let pack = spells
            .iter()
            .find(|s| s.name == "Pack Chloroplast")
            .expect("Pack Chloroplast");
        // spells_us: formula 9 (level×2+10), cap 140
        assert_eq!(pack.duration_formula, "level_x2_plus_10");
        assert_eq!(pack.max_ticks, 140);
        assert_eq!(duration_seconds(pack, 42, 0), 564); // 94 ticks
    }

    #[test]
    fn classic_formula_ids_1_through_15() {
        // Formula 1: level/2
        assert_eq!(ticks_from_formula("level_div_2", 40, 0), 20);
        // Formula 4 / 5: fixed constants
        assert_eq!(ticks_from_formula("fixed_50", 1, 0), 50);
        assert_eq!(ticks_from_formula("fixed_2", 99, 0), 2);
        // Formula 6: level/2+2
        assert_eq!(ticks_from_formula("level_div_2_plus_2", 20, 0), 12);
        // Formula 7 / 8
        assert_eq!(ticks_from_formula("level", 35, 0), 35);
        assert_eq!(ticks_from_formula("level_plus_10", 35, 0), 45);
        // Formula 12: level/4 (or 1 if level≤7)
        assert_eq!(ticks_from_formula("level_div_4", 4, 0), 1);
        assert_eq!(ticks_from_formula("level_div_4", 40, 0), 10);
        // Formula 13 / 14 / 15
        assert_eq!(ticks_from_formula("level_x4_plus_10", 10, 0), 50);
        assert_eq!(ticks_from_formula("level_plus_2_x5", 10, 0), 60);
        assert_eq!(ticks_from_formula("level_plus_10_x10", 5, 0), 150);
        // Permanent markers yield 0 ticks (no overlay timer)
        assert_eq!(ticks_from_formula("f50", 50, 0), 0);
        assert_eq!(ticks_from_formula("permanent", 50, 0), 0);
    }

    #[test]
    fn clarity_stays_fixed_at_cap() {
        // Client formula 3 (level×30) already hits 270-tick cap by Enchanter 26.
        let spells = load_spells().expect("spells");
        let clarity = spells.iter().find(|s| s.name == "Clarity").unwrap();
        assert_eq!(clarity.duration_formula, "fixed");
        assert_eq!(clarity.max_ticks, 270);
        assert_eq!(clarity.land_you, "A cool breeze slips through your mind");
        assert_eq!(duration_seconds(clarity, 26, 0), 1620);
    }

    #[test]
    fn gift_of_magic_from_spells_us_cap() {
        // spells_us: formula 3 (level×30), cap 600 — at Enc 34 already capped (1h).
        let spells = load_spells().expect("spells");
        let gom = spells
            .iter()
            .find(|s| s.name == "Gift of Magic")
            .expect("Gift of Magic");
        assert_eq!(gom.duration_formula, "fixed");
        assert_eq!(gom.base_ticks, 600);
        assert_eq!(gom.max_ticks, 600);
        assert_eq!(gom.spellicon, "H");
        assert_eq!(
            gom.land_you,
            "Your thoughts begin to race and flow faster"
        );
        assert_eq!(gom.wear_off_you, "Your gift of magic fades");
        assert!(gom.watched_by_default);
        assert_eq!(duration_seconds(gom, 34, 0), 3600);
    }

    #[test]
    fn shield_of_thorns_resolves_from_cast_name() {
        // Wiki used "Shield of Thorns (Spell)"; cast log is bare "Shield of Thorns".
        let spells = load_spells().expect("spells");
        let (spell, tier) = resolve_cast_spell(&spells, "Shield of Thorns").unwrap();
        assert_eq!(spell.name, "Shield of Thorns");
        assert_eq!(tier, 0);
        assert_eq!(spell.land_you, "You are surrounded by a thorny barrier");
        assert_eq!(spell.wear_off_you, "The brambles fall away");
        assert_eq!(duration_seconds(spell, 47, 0), 150 * 6);
    }

    #[test]
    fn is_watched_treats_spell_suffix_as_alias() {
        let mut config = AppConfig::default();
        // Stale key from wiki title; bare name left false after seed.
        config.watched.insert("Shield of Thorns".into(), false);
        config
            .watched
            .insert("Shield of Thorns (Spell)".into(), true);
        assert!(is_watched(&config, "Shield of Thorns"));
        assert!(is_watched(&config, "Shield of Thorns (Spell)"));
    }

    #[test]
    fn expiry_warn_secs_clamped_and_defaults() {
        let mut ov = OverlayAppearance::default();
        assert!(ov.flash_expiry_warn);
        assert!(!ov.verbal_expiry_warn);
        assert_eq!(ov.expiry_warn_secs, 30);
        assert_eq!(ov.expiry_warn_secs_clamped(), 30);
        ov.expiry_warn_secs = 0;
        assert_eq!(ov.expiry_warn_secs_clamped(), 1);
        ov.expiry_warn_secs = 99;
        assert_eq!(ov.expiry_warn_secs_clamped(), 30);
        let mut cfg = AppConfig::default();
        cfg.overlay.expiry_warn_secs = 0;
        normalize_config(&mut cfg);
        assert_eq!(cfg.overlay.expiry_warn_secs, 1);
    }

    #[test]
    fn spell_casting_reinforcement_rank_percents() {
        assert_eq!(spell_casting_reinforcement_pct(0), 0.0);
        assert_eq!(spell_casting_reinforcement_pct(1), 5.0);
        assert_eq!(spell_casting_reinforcement_pct(2), 15.0);
        assert_eq!(spell_casting_reinforcement_pct(3), 30.0);
        assert_eq!(spell_casting_reinforcement_pct(4), 50.0);
        assert_eq!(spell_casting_reinforcement_pct(99), 50.0);
    }

    #[test]
    fn spell_casting_reinforcement_extends_buffs_not_debuffs() {
        let celerity = haste("Celerity", "level_x3_plus_10", 160);
        // L50 cap 160 ticks = 960s; rank 4 → 240 ticks = 1440s.
        assert_eq!(duration_seconds(&celerity, 50, 0), 960);
        assert_eq!(duration_seconds_with_aa(&celerity, 50, 0, 4), 1440);
        assert_eq!(duration_seconds_with_aa(&celerity, 50, 0, 1), 1008); // 160 * 1.05
        assert!(!spell_eligible_for_reinforcement(&mez()));
        assert_eq!(duration_seconds_with_aa(&mez(), 50, 0, 4), 24);
    }

    #[test]
    fn spell_casting_reinforcement_skips_invulnerability() {
        let spells = load_spells().expect("spells");
        let aura = spells
            .iter()
            .find(|s| s.name == "Divine Aura")
            .expect("Divine Aura");
        let barrier = spells
            .iter()
            .find(|s| s.name == "Divine Barrier")
            .expect("Divine Barrier");
        let harm = spells
            .iter()
            .find(|s| s.name == "Harmshield")
            .expect("Harmshield");
        assert!(!spell_eligible_for_reinforcement(aura));
        assert!(!spell_eligible_for_reinforcement(barrier));
        assert!(!spell_eligible_for_reinforcement(harm));
        assert_eq!(duration_seconds(aura, 50, 0), duration_seconds_with_aa(aura, 50, 0, 4));
        assert_eq!(
            duration_seconds(barrier, 50, 0),
            duration_seconds_with_aa(barrier, 50, 0, 4)
        );
    }

    #[test]
    fn spell_casting_reinforcement_rank_clamped_on_normalize() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.spell_casting_reinforcement, 0);
        cfg.spell_casting_reinforcement = 99;
        normalize_config(&mut cfg);
        assert_eq!(cfg.spell_casting_reinforcement, 4);
    }
}
