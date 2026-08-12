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
  /**
   * Web Speech `SpeechSynthesisVoice.voiceURI` for announcements.
   * Empty string = system default voice.
   */
  voice_uri: string;
  /** Show the independent respawn overlay window. */
  show_respawn_window: boolean;
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

export interface ThemePreset {
  id: string;
  label: string;
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
    | "voice_uri"
    | "show_respawn_window"
    | "track_all_kills"
    | "show_window_border"
  >;
}

export const TIMER_SIZES = ["minimal", "small", "normal", "large"] as const;

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
  voice_uri: "",
  show_respawn_window: true,
  track_all_kills: true,
  show_window_border: false,
};

/** Clamp recently-wore-off retention to the supported slider range. */
export function clampRecentlyWoreOffSecs(secs: number | undefined | null): number {
  const n = Number(secs);
  if (!Number.isFinite(n)) return DEFAULT_OVERLAY.recently_wore_off_secs;
  return Math.min(300, Math.max(15, Math.round(n)));
}

export function formatRecentlyWoreOffLabel(secs: number): string {
  const s = clampRecentlyWoreOffSecs(secs);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  if (rem === 0) return m === 1 ? "1 min" : `${m} min`;
  return `${m}m ${rem}s`;
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
    appearance: {
      theme: "berry",
      text_color: "#f6ebf1",
      panel_color: "#160e14",
      buff_color: "#7eb8a2",
      debuff_color: "#c45c8a",
      dot_color: "#d4a05a",
      panel_opacity: 0.82,
      bar_opacity: 1.0,
    },
  },
  {
    id: "parchment",
    label: "Classic parchment",
    appearance: {
      theme: "parchment",
      text_color: "#3a2a18",
      panel_color: "#e4d2a8",
      buff_color: "#4a7a48",
      debuff_color: "#9a3c2e",
      dot_color: "#b07028",
      panel_opacity: 0.92,
      bar_opacity: 1.0,
    },
  },
  {
    id: "eq-chrome",
    label: "Dark blue EQ chrome",
    appearance: {
      theme: "eq-chrome",
      text_color: "#d8e2f0",
      panel_color: "#152238",
      buff_color: "#5a9ec8",
      debuff_color: "#c45c5c",
      dot_color: "#c9a04a",
      panel_opacity: 0.88,
      bar_opacity: 1.0,
    },
  },
  {
    id: "velious",
    label: "Velious ice",
    appearance: {
      theme: "velious",
      text_color: "#e8f4ff",
      panel_color: "#163448",
      buff_color: "#7ec8e0",
      debuff_color: "#6a8ab8",
      dot_color: "#a8d0e8",
      panel_opacity: 0.85,
      bar_opacity: 1.0,
    },
  },
  {
    id: "kunark",
    label: "Kunark / swamp",
    appearance: {
      theme: "kunark",
      text_color: "#e4efd4",
      panel_color: "#1a2814",
      buff_color: "#6a9a4a",
      debuff_color: "#8a5a3a",
      dot_color: "#b09040",
      panel_opacity: 0.88,
      bar_opacity: 1.0,
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
    font_family: current.font_family,
    show_icons: current.show_icons,
    right_click_dismiss: current.right_click_dismiss,
    show_recently_wore_off: current.show_recently_wore_off,
    recently_wore_off_secs: current.recently_wore_off_secs,
    separate_enemy_window: current.separate_enemy_window,
    self_buffs_only: current.self_buffs_only,
    hide_other_pets: current.hide_other_pets,
    voice_announcements: current.voice_announcements,
    voice_uri: current.voice_uri,
    show_respawn_window: current.show_respawn_window,
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

export function iconImgHtml(spellicon: string | undefined | null, className = "spell-icon"): string {
  const src = iconUrl(spellicon);
  if (!src) return `<span class="${className} spell-icon-missing" aria-hidden="true"></span>`;
  return `<img class="${className}" src="${src}" alt="" width="20" height="20" loading="lazy" onerror="this.classList.add('is-missing');this.removeAttribute('src')" />`;
}
