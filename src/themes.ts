/** EQ-flavored overlay appearance presets. */

export interface OverlayAppearance {
  text_color: string;
  panel_color: string;
  buff_color: string;
  debuff_color: string;
  dot_color: string;
  panel_opacity: number;
  bar_opacity: number;
  timer_size: string;
  theme: string;
  /** Primary Windows font family name for overlay text (see OVERLAY_FONTS). */
  font_family: string;
  show_icons: boolean;
  /** Right-click overlay timer to dismiss (requires unlocked overlay). */
  right_click_dismiss: boolean;
  /** Show muted recently-wore-off rows after a timer ends. */
  show_recently_wore_off: boolean;
  /** How long recently-wore-off rows stay (seconds). Clamped 15..=300. */
  recently_wore_off_secs: number;
  /** Split enemy debuff/DoT timers into a second overlay window. */
  separate_enemy_window: boolean;
  /** Main overlay: only show timers on You (+ my pet when set). Enemies overlay unchanged. */
  self_buffs_only: boolean;
  /** Hide timers on other pets; keep You, my pet, and non-pet allies. */
  hide_other_pets: boolean;
  /** Speak when a timer wears off or is dismissed (main overlay). */
  voice_announcements: boolean;
  /** Speak when your charm ends (`Your charm spell has worn off`), even if
   * general wear-off announcements are off.
   */
  charm_break_alerts: boolean;
  /** Speak/show when self invis fades (`You feel yourself starting to appear`)
   * or drops (`You appear`, Camouflage, IVU, IVA).
   */
  invis_break_alerts: boolean;
  /**
   * Web Speech `SpeechSynthesisVoice.voiceURI` for announcements.
   * Empty string = system default voice.
   */
  voice_uri: string;
  /** TTS volume 0–1 (SpeechSynthesisUtterance.volume). */
  voice_volume: number;
  /** Flash (urgent pulse) when a timer is within expiry_warn_secs of ending. */
  flash_expiry_warn: boolean;
  /** Speak when a timer crosses into the expiry-warn window (main overlay). */
  verbal_expiry_warn: boolean;
  /** Shared lead time (seconds) for flash + verbal pre-expiry alerts. Clamped 1..=120. */
  expiry_warn_secs: number;
  /** Show the independent respawn overlay window. */
  show_respawn_window: boolean;
  /** Show the fading-message alert overlay window. */
  show_alert_window: boolean;
  /** Show the compact DPS meter overlay window. */
  show_meter_window: boolean;
  /** How long alert overlay toasts stay (seconds). Clamped 2..=15. */
  alert_secs: number;
  /** Alert overlay font; empty = same as overlay font_family. */
  alert_font_family: string;
  /** Alert toast size: small | normal | large | huge. */
  alert_size: string;
  /** Charm-break toast title color. */
  alert_charm_color: string;
  /** Invis / IVU / IVA toast title color. */
  alert_invis_color: string;
  /** Start a zone-default respawn timer on every kill (rares still use overrides). */
  track_all_kills: boolean;
  /**
   * Deprecated / ignored: native DWM border/shadow follows overlay lock
   * (on when unlocked, off when locked). Kept for config backwards-compat.
   */
  show_window_border: boolean;
}

export interface OverlayFontOption {
  /** Stored in `overlay.font_family` / AppConfig. */
  id: string;
  label: string;
  /** Full CSS font-family stack for --overlay-font. */
  css: string;
}

/** Curated readable Windows system fonts for the overlay. */
export const OVERLAY_FONTS: OverlayFontOption[] = [
  { id: "Segoe UI", label: "Segoe UI (default)", css: '"Segoe UI", sans-serif' },
  {
    id: "Cascadia Mono",
    label: "Cascadia Mono",
    css: '"Cascadia Mono", Consolas, monospace',
  },
  { id: "Consolas", label: "Consolas", css: 'Consolas, "Courier New", monospace' },
  {
    id: "Courier New",
    label: "Courier New",
    css: '"Courier New", Courier, monospace',
  },
  {
    id: "Lucida Console",
    label: "Lucida Console",
    css: '"Lucida Console", "Courier New", monospace',
  },
  { id: "Georgia", label: "Georgia", css: 'Georgia, "Times New Roman", serif' },
  {
    id: "Palatino Linotype",
    label: "Palatino Linotype",
    css: '"Palatino Linotype", "Book Antiqua", Palatino, serif',
  },
  { id: "Trebuchet MS", label: "Trebuchet MS", css: '"Trebuchet MS", sans-serif' },
  { id: "Arial", label: "Arial", css: "Arial, Helvetica, sans-serif" },
  { id: "Verdana", label: "Verdana", css: "Verdana, Geneva, sans-serif" },
  { id: "Calibri", label: "Calibri", css: 'Calibri, "Segoe UI", sans-serif' },
  { id: "Tahoma", label: "Tahoma", css: "Tahoma, sans-serif" },
];

export type ThemeGroup = "classic" | "fantasy" | "terminal";

export const THEME_GROUPS: { id: ThemeGroup; label: string }[] = [
  { id: "classic", label: "Classic" },
  { id: "fantasy", label: "Fantasy" },
  { id: "terminal", label: "Terminal / retro" },
];

export interface ThemePreset {
  id: string;
  label: string;
  group: ThemeGroup;
  appearance: Omit<
    OverlayAppearance,
    | "timer_size"
    | "font_family"
    | "show_icons"
    | "right_click_dismiss"
    | "show_recently_wore_off"
    | "recently_wore_off_secs"
    | "separate_enemy_window"
    | "self_buffs_only"
    | "hide_other_pets"
    | "voice_announcements"
    | "charm_break_alerts"
    | "invis_break_alerts"
    | "voice_uri"
    | "voice_volume"
    | "flash_expiry_warn"
    | "verbal_expiry_warn"
    | "expiry_warn_secs"
    | "show_respawn_window"
    | "show_alert_window"
    | "show_meter_window"
    | "alert_secs"
    | "alert_font_family"
    | "alert_size"
    | "alert_charm_color"
    | "alert_invis_color"
    | "track_all_kills"
    | "show_window_border"
  > & {
    /** When set, picking this theme also switches overlay font. */
    font_family?: string;
  };
}

export const TIMER_SIZES = ["minimal", "small", "normal", "large"] as const;

export const ALERT_SIZES = ["small", "normal", "large", "huge"] as const;

export const DEFAULT_OVERLAY: OverlayAppearance = {
  text_color: "#f6ebf1",
  panel_color: "#160e14",
  buff_color: "#7eb8a2",
  debuff_color: "#c45c8a",
  dot_color: "#d4a05a",
  panel_opacity: 0.82,
  bar_opacity: 1.0,
  timer_size: "normal",
  theme: "berry",
  font_family: "Segoe UI",
  show_icons: true,
  right_click_dismiss: true,
  show_recently_wore_off: true,
  recently_wore_off_secs: 300,
  separate_enemy_window: false,
  self_buffs_only: false,
  hide_other_pets: false,
  voice_announcements: true,
  charm_break_alerts: true,
  invis_break_alerts: true,
  voice_uri: "",
  voice_volume: 1,
  flash_expiry_warn: true,
  verbal_expiry_warn: false,
  expiry_warn_secs: 10,
  show_respawn_window: true,
  show_alert_window: true,
  show_meter_window: false,
  alert_secs: 5,
  alert_font_family: "",
  alert_size: "large",
  alert_charm_color: "#ff3b3b",
  alert_invis_color: "#6ec8ff",
  track_all_kills: true,
  show_window_border: false,
};

/** Clamp recently-wore-off retention to the supported slider range. */
export function clampRecentlyWoreOffSecs(secs: number | undefined | null): number {
  const n = Number(secs);
  if (!Number.isFinite(n)) return DEFAULT_OVERLAY.recently_wore_off_secs;
  return Math.min(300, Math.max(15, Math.round(n)));
}

/** Pre-expiry flash/verbal lead-time bounds (seconds). */
export const EXPIRY_WARN_SECS_MIN = 1;
export const EXPIRY_WARN_SECS_MAX = 120;

/** Clamp pre-expiry warn lead time to 1..=120 seconds. */
export function clampExpiryWarnSecs(secs: number | undefined | null): number {
  const n = Number(secs);
  if (!Number.isFinite(n)) return DEFAULT_OVERLAY.expiry_warn_secs;
  return Math.min(EXPIRY_WARN_SECS_MAX, Math.max(EXPIRY_WARN_SECS_MIN, Math.round(n)));
}

/**
 * Effective warn window in ms. Caps at half the timer so a 60s proc buff
 * with a 30s setting does not flash for most of its life.
 */
export function expiryWarnThresholdMs(durationMs: number, warnSecs: number): number {
  const warnMs = clampExpiryWarnSecs(warnSecs) * 1000;
  if (!(durationMs > 0)) return warnMs;
  return Math.max(1000, Math.min(warnMs, durationMs * 0.5));
}

export function formatRecentlyWoreOffLabel(secs: number): string {
  const s = clampRecentlyWoreOffSecs(secs);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  if (rem === 0) return m === 1 ? "1 min" : `${m} min`;
  return `${m}m ${rem}s`;
}

export function formatExpiryWarnLabel(secs: number): string {
  return `${clampExpiryWarnSecs(secs)}s`;
}

/** Alert overlay toast lifetime bounds (seconds). */
export const ALERT_SECS_MIN = 2;
export const ALERT_SECS_MAX = 15;

export function clampAlertSecs(secs: number | undefined | null): number {
  const n = Number(secs);
  if (!Number.isFinite(n)) return DEFAULT_OVERLAY.alert_secs;
  return Math.min(ALERT_SECS_MAX, Math.max(ALERT_SECS_MIN, Math.round(n)));
}

export function formatAlertSecsLabel(secs: number): string {
  return `${clampAlertSecs(secs)}s`;
}

export function clampAlertSize(size: string | undefined | null): string {
  const s = (size || DEFAULT_OVERLAY.alert_size).trim().toLowerCase();
  return (ALERT_SIZES as readonly string[]).includes(s) ? s : DEFAULT_OVERLAY.alert_size;
}

export function overlayAlertFontCss(
  alertFont: string | undefined | null,
  overlayFont: string | undefined | null
): string {
  const id = (alertFont || "").trim();
  if (!id) return overlayFontCss(overlayFont);
  return overlayFontCss(id);
}

export function clampHexColor(value: string | undefined | null, fallback: string): string {
  const v = (value || "").trim();
  if (/^#[0-9a-fA-F]{6}$/.test(v)) return v;
  return fallback;
}

export function overlayFontCss(fontFamily: string | undefined | null): string {
  const id = (fontFamily || DEFAULT_OVERLAY.font_family).trim();
  const found = OVERLAY_FONTS.find((f) => f.id.toLowerCase() === id.toLowerCase());
  return found?.css ?? OVERLAY_FONTS[0].css;
}

export const THEME_PRESETS: ThemePreset[] = [
  {
    id: "berry",
    label: "Berry (default)",
    group: "classic",
    appearance: {
      theme: "berry",
      text_color: "#f6ebf1",
      panel_color: "#160e14",
      buff_color: "#7eb8a2",
      debuff_color: "#c45c8a",
      dot_color: "#d4a05a",
      panel_opacity: 0.82,
      bar_opacity: 1.0,
      font_family: "Segoe UI",
    },
  },
  {
    id: "parchment",
    label: "Classic parchment",
    group: "classic",
    appearance: {
      theme: "parchment",
      text_color: "#3a2a18",
      panel_color: "#e4d2a8",
      buff_color: "#4a7a48",
      debuff_color: "#9a3c2e",
      dot_color: "#b07028",
      panel_opacity: 0.92,
      bar_opacity: 1.0,
      font_family: "Georgia",
    },
  },
  {
    id: "eq-chrome",
    label: "Dark blue EQ chrome",
    group: "classic",
    appearance: {
      theme: "eq-chrome",
      text_color: "#d8e2f0",
      panel_color: "#152238",
      buff_color: "#5a9ec8",
      debuff_color: "#c45c5c",
      dot_color: "#c9a04a",
      panel_opacity: 0.88,
      bar_opacity: 1.0,
      font_family: "Segoe UI",
    },
  },
  {
    id: "velious",
    label: "Velious ice",
    group: "classic",
    appearance: {
      theme: "velious",
      text_color: "#e8f4ff",
      panel_color: "#163448",
      buff_color: "#7ec8e0",
      debuff_color: "#6a8ab8",
      dot_color: "#a8d0e8",
      panel_opacity: 0.85,
      bar_opacity: 1.0,
      font_family: "Segoe UI",
    },
  },
  {
    id: "kunark",
    label: "Kunark / swamp",
    group: "classic",
    appearance: {
      theme: "kunark",
      text_color: "#e4efd4",
      panel_color: "#1a2814",
      buff_color: "#6a9a4a",
      debuff_color: "#8a5a3a",
      dot_color: "#b09040",
      panel_opacity: 0.88,
      bar_opacity: 1.0,
      font_family: "Segoe UI",
    },
  },
  {
    id: "ironkeep",
    label: "Ironkeep",
    group: "fantasy",
    appearance: {
      theme: "ironkeep",
      text_color: "#e6d9c4",
      panel_color: "#14110f",
      buff_color: "#7a9a62",
      debuff_color: "#9c3228",
      dot_color: "#c9a227",
      panel_opacity: 0.92,
      bar_opacity: 1.0,
      font_family: "Palatino Linotype",
    },
  },
  {
    id: "grimoire",
    label: "Grimoire",
    group: "fantasy",
    appearance: {
      theme: "grimoire",
      text_color: "#d8c9a0",
      panel_color: "#100e0c",
      buff_color: "#5e7d68",
      debuff_color: "#7a2e3c",
      dot_color: "#b8862a",
      panel_opacity: 0.94,
      bar_opacity: 1.0,
      font_family: "Palatino Linotype",
    },
  },
  {
    id: "phosphor",
    label: "Phosphor CRT",
    group: "terminal",
    appearance: {
      theme: "phosphor",
      text_color: "#8eef8e",
      panel_color: "#071207",
      buff_color: "#2ee56a",
      debuff_color: "#e85a4a",
      dot_color: "#d4e04a",
      panel_opacity: 0.92,
      bar_opacity: 1.0,
      font_family: "Cascadia Mono",
    },
  },
  {
    id: "amber",
    label: "Amber CRT",
    group: "terminal",
    appearance: {
      theme: "amber",
      text_color: "#ffb000",
      panel_color: "#140e04",
      buff_color: "#e89a00",
      debuff_color: "#ff6030",
      dot_color: "#ffd24a",
      panel_opacity: 0.92,
      bar_opacity: 1.0,
      font_family: "Cascadia Mono",
    },
  },
  {
    id: "vga",
    label: "VGA night",
    group: "terminal",
    appearance: {
      theme: "vga",
      text_color: "#c8c8c8",
      panel_color: "#000010",
      buff_color: "#55ffff",
      debuff_color: "#ff55ff",
      dot_color: "#ffff55",
      panel_opacity: 0.92,
      bar_opacity: 1.0,
      font_family: "Lucida Console",
    },
  },
];

export function applyThemePreset(
  current: OverlayAppearance,
  themeId: string
): OverlayAppearance {
  const preset = THEME_PRESETS.find((t) => t.id === themeId);
  if (!preset) return { ...current, theme: themeId };
  return {
    ...current,
    ...preset.appearance,
    timer_size: current.timer_size,
    font_family: preset.appearance.font_family ?? current.font_family,
    show_icons: current.show_icons,
    right_click_dismiss: current.right_click_dismiss,
    show_recently_wore_off: current.show_recently_wore_off,
    recently_wore_off_secs: current.recently_wore_off_secs,
    separate_enemy_window: current.separate_enemy_window,
    self_buffs_only: current.self_buffs_only,
    hide_other_pets: current.hide_other_pets,
    voice_announcements: current.voice_announcements,
    charm_break_alerts: current.charm_break_alerts,
    invis_break_alerts: current.invis_break_alerts,
    voice_uri: current.voice_uri,
    voice_volume: current.voice_volume,
    flash_expiry_warn: current.flash_expiry_warn,
    verbal_expiry_warn: current.verbal_expiry_warn,
    expiry_warn_secs: current.expiry_warn_secs,
    show_respawn_window: current.show_respawn_window,
    show_alert_window: current.show_alert_window,
    show_meter_window: current.show_meter_window,
    alert_secs: current.alert_secs,
    alert_font_family: current.alert_font_family,
    alert_size: current.alert_size,
    alert_charm_color: current.alert_charm_color,
    alert_invis_color: current.alert_invis_color,
    track_all_kills: current.track_all_kills,
    show_window_border: current.show_window_border,
  };
}

export function iconUrl(spellicon: string | undefined | null): string | null {
  if (!spellicon) return null;
  const id = String(spellicon).trim();
  if (!id || !/^[\w.-]+$/.test(id)) return null;
  return `./icons/spellicon_${id}.png`;
}

/** Icon srcs that 404'd this session — skip retry so missing files don't flash. */
const failedIconSrcs = new Set<string>();

export function iconImgHtml(spellicon: string | undefined | null, className = "spell-icon"): string {
  const src = iconUrl(spellicon);
  if (!src || failedIconSrcs.has(src)) {
    return `<span class="${className} spell-icon-missing" aria-hidden="true"></span>`;
  }
  return `<img class="${className}" src="${src}" alt="" width="20" height="20" />`;
}

/** Capture icon 404s once; later renders use a placeholder instead of retrying. */
export function bindIconErrorHandling(root: ParentNode = document): void {
  root.addEventListener(
    "error",
    (e) => {
      const t = e.target;
      if (!(t instanceof HTMLImageElement)) return;
      if (!t.classList.contains("spell-icon") && !t.classList.contains("oicon")) return;
      const src = t.getAttribute("src");
      if (src) failedIconSrcs.add(src);
      const span = document.createElement("span");
      span.className = `${t.className} spell-icon-missing`;
      span.setAttribute("aria-hidden", "true");
      t.replaceWith(span);
    },
    true
  );
}
