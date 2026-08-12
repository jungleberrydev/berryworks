import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  DEFAULT_OVERLAY,
  OVERLAY_FONTS,
  TIMER_SIZES,
  applyThemePreset,
  clampRecentlyWoreOffSecs,
  formatRecentlyWoreOffLabel,
  iconImgHtml,
  type OverlayAppearance,
} from "./themes";
import { PET_NAME_HINT } from "./pets";

export interface SpellClassLevel {
  class: string;
  level: number;
}

export interface SpellDef {
  name: string;
  category: string;
  duration_formula: string;
  base_ticks: number;
  max_ticks: number;
  tier_duration_pct: number;
  land_other: string;
  land_you: string;
  wear_off_you: string;
  watched_by_default: boolean;
  classes?: SpellClassLevel[];
  spellicon?: string;
}

export interface AppConfig {
  log_path: string;
  character_level: number;
  /** Exact pet target string from logs (e.g. Gastik or Jungleberry pet). */
  my_pet_name?: string;
  spell_tiers: Record<string, number>;
  watched: Record<string, boolean>;
  watched_rares?: Record<string, boolean>;
  camp_overrides?: Record<string, number>;
  respawn_zone?: string;
  loot_tracking?: boolean;
  loot_sync_enabled?: boolean;
  loot_sync_url?: string;
  loot_sync_key?: string;
  loot_contributor_id?: string;
  overlay_locked: boolean;
  overlay?: OverlayAppearance;
}

export interface LootMobRow {
  zone: string;
  mob: string;
  mob_key: string;
  kills: number;
  unique_items: number;
  coin_avg_copper: number;
}

export interface LootItemRow {
  zone: string;
  mob: string;
  mob_key: string;
  item: string;
  times: number;
  quantity: number;
  kills: number;
  rate: number | null;
}

export interface LootSnapshot {
  total_kills: number;
  total_item_appearances: number;
  mob_count: number;
  zone: string | null;
  mobs: LootMobRow[];
  items: LootItemRow[];
}

export interface RareCamp {
  id: string;
  label: string;
  npc_names: string[];
  respawn_secs: number;
  watched_by_default?: boolean;
}

export interface ZoneCamps {
  id: string;
  names: string[];
  default_respawn_secs: number;
  rares: RareCamp[];
}

export interface CampsFile {
  global_default_respawn_secs: number;
  zones: ZoneCamps[];
}

export interface ActiveTimer {
  id: string;
  spell: string;
  target: string;
  category: string;
  started_at: string;
  ends_at: string;
  duration_secs: number;
}

const OTHER_CLASS = "Other";
const EXPANDED_CLASS_KEY = "berry-timers-expanded-class";
const DEFAULT_EXPANDED_CLASS = "Enchanter";

let spells: SpellDef[] = [];
let camps: CampsFile | null = null;
let config: AppConfig | null = null;
let spellSearch = "";
let lootSearch = "";
let lootView: "mobs" | "items" = "mobs";
let lootSnapshot: LootSnapshot | null = null;
let appearanceSaveTimer: ReturnType<typeof setTimeout> | null = null;
let lootSearchTimer: ReturnType<typeof setTimeout> | null = null;

function $(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`Missing #${id}`);
  return el;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function formatRemain(endsAt: string): { text: string; pct: number } {
  const end = new Date(endsAt).getTime();
  const now = Date.now();
  const ms = Math.max(0, end - now);
  const secs = Math.ceil(ms / 1000);
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  const text = `${m}:${s.toString().padStart(2, "0")}`;
  return { text, pct: ms };
}

/** Alphabetical class order; "Other" always last. */
function compareClasses(a: string, b: string): number {
  if (a === OTHER_CLASS && b !== OTHER_CLASS) return 1;
  if (b === OTHER_CLASS && a !== OTHER_CLASS) return -1;
  return a.localeCompare(b);
}

function getRememberedClass(): string {
  try {
    return localStorage.getItem(EXPANDED_CLASS_KEY) || DEFAULT_EXPANDED_CLASS;
  } catch {
    return DEFAULT_EXPANDED_CLASS;
  }
}

function rememberClass(name: string) {
  try {
    localStorage.setItem(EXPANDED_CLASS_KEY, name);
  } catch {
    /* ignore */
  }
}

function overlayOf(cfg: AppConfig | null): OverlayAppearance {
  return { ...DEFAULT_OVERLAY, ...(cfg?.overlay ?? {}) };
}

function spellIconByName(name: string): string | undefined {
  return spells.find((s) => s.name === name)?.spellicon;
}

type GroupedSpell = { spell: SpellDef; level: number };

/** Build Class → Level → spells; multi-class spells appear under every class. */
function groupSpellsByClassLevel(list: SpellDef[]): Map<string, Map<number, GroupedSpell[]>> {
  const byClass = new Map<string, Map<number, GroupedSpell[]>>();

  const ensure = (cls: string, level: number) => {
    let levels = byClass.get(cls);
    if (!levels) {
      levels = new Map();
      byClass.set(cls, levels);
    }
    let rows = levels.get(level);
    if (!rows) {
      rows = [];
      levels.set(level, rows);
    }
    return rows;
  };

  for (const spell of list) {
    const entries = spell.classes?.length
      ? spell.classes
      : [{ class: OTHER_CLASS, level: 0 }];
    for (const entry of entries) {
      const cls = entry.class?.trim() || OTHER_CLASS;
      const level = Number.isFinite(entry.level) ? entry.level : 0;
      ensure(cls, level).push({ spell, level });
    }
  }

  for (const levels of byClass.values()) {
    for (const rows of levels.values()) {
      rows.sort((a, b) => a.spell.name.localeCompare(b.spell.name));
    }
  }

  return byClass;
}

function uniqueSpellNamesInClass(levels: Map<number, GroupedSpell[]>): string[] {
  const names = new Set<string>();
  for (const rows of levels.values()) {
    for (const { spell } of rows) names.add(spell.name);
  }
  return [...names];
}

function syncWatchCheckboxes(spellName: string, checked: boolean) {
  document
    .querySelectorAll<HTMLInputElement>(`.watch-toggle[data-spell="${CSS.escape(spellName)}"]`)
    .forEach((el) => {
      el.checked = checked;
    });
}

function setClassWatched(className: string, watched: boolean) {
  if (!config) return;
  const details = document.querySelector<HTMLDetailsElement>(
    `details.class-section[data-class="${CSS.escape(className)}"]`
  );
  if (!details) return;
  const seen = new Set<string>();
  details.querySelectorAll<HTMLInputElement>(".watch-toggle").forEach((el) => {
    const name = el.dataset.spell!;
    if (seen.has(name)) return;
    seen.add(name);
    config!.watched[name] = watched;
    syncWatchCheckboxes(name, watched);
  });
}

function renderSpellList() {
  if (!config) return;
  const list = $("spell-list");
  list.innerHTML = "";

  const query = spellSearch.trim().toLowerCase();
  const filtered = query
    ? spells.filter((s) => s.name.toLowerCase().includes(query))
    : spells;

  const grouped = groupSpellsByClassLevel(filtered);
  const classNames = [...grouped.keys()].sort(compareClasses);
  const expanded = getRememberedClass();
  const searching = query.length > 0;

  if (!classNames.length) {
    list.innerHTML = `<div class="spell-empty">No spells match “${escapeHtml(spellSearch.trim())}”</div>`;
    return;
  }

  for (const className of classNames) {
    const levels = grouped.get(className)!;
    const levelNums = [...levels.keys()].sort((a, b) => a - b);
    const spellCount = uniqueSpellNamesInClass(levels).length;

    const details = document.createElement("details");
    details.className = "class-section";
    details.open = searching || className === expanded;
    details.dataset.class = className;

    const summary = document.createElement("summary");
    summary.className = "class-summary";
    summary.innerHTML = `
      <span class="class-name">${escapeHtml(className)}</span>
      <span class="class-actions">
        <button type="button" class="class-bulk" data-bulk="enable" data-class="${escapeHtml(className)}">Enable all</button>
        <button type="button" class="class-bulk" data-bulk="disable" data-class="${escapeHtml(className)}">Disable all</button>
      </span>
      <span class="class-count">${spellCount}</span>
    `;
    details.appendChild(summary);

    const body = document.createElement("div");
    body.className = "class-body";

    for (const level of levelNums) {
      const rows = levels.get(level)!;
      const levelBlock = document.createElement("div");
      levelBlock.className = "level-section";

      const levelLabel =
        className === OTHER_CLASS && level === 0
          ? "Unknown level"
          : `Level ${level}`;
      levelBlock.innerHTML = `<div class="level-heading">${escapeHtml(levelLabel)}</div>`;

      for (const { spell } of rows) {
        const watched = config.watched[spell.name] ?? spell.watched_by_default;
        const row = document.createElement("div");
        row.className = "spell-row";
        row.innerHTML = `
          <label class="spell-watch">
            <input type="checkbox" data-spell="${escapeHtml(spell.name)}" class="watch-toggle" ${watched ? "checked" : ""} />
            ${iconImgHtml(spell.spellicon)}
            <span class="spell-name">${escapeHtml(spell.name)}</span>
            <span class="badge ${escapeHtml(spell.category)}">${escapeHtml(spell.category)}</span>
          </label>
        `;
        levelBlock.appendChild(row);
      }
      body.appendChild(levelBlock);
    }

    details.appendChild(body);
    list.appendChild(details);
  }

  list.querySelectorAll<HTMLDetailsElement>("details.class-section").forEach((el) => {
    el.addEventListener("toggle", () => {
      if (el.open) {
        rememberClass(el.dataset.class || DEFAULT_EXPANDED_CLASS);
        if (!searching) {
          list.querySelectorAll<HTMLDetailsElement>("details.class-section").forEach((other) => {
            if (other !== el) other.open = false;
          });
        }
      }
    });
  });

  list.querySelectorAll<HTMLButtonElement>("button.class-bulk").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      const cls = btn.dataset.class!;
      const enable = btn.dataset.bulk === "enable";
      setClassWatched(cls, enable);
    });
  });

  list.querySelectorAll<HTMLInputElement>(".watch-toggle").forEach((el) => {
    el.addEventListener("change", () => {
      const name = el.dataset.spell!;
      syncWatchCheckboxes(name, el.checked);
      if (config) config.watched[name] = el.checked;
    });
  });
}

function formatRespawnSecs(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  if (m >= 60) {
    const h = Math.floor(m / 60);
    const rm = m % 60;
    return rm ? `${h}h ${rm}m` : `${h}h`;
  }
  return s ? `${m}m ${s}s` : `${m}m`;
}

function fillZoneSelect(select: HTMLSelectElement, selected: string) {
  const current = selected.trim();
  select.innerHTML = `<option value="">— Select zone —</option>`;
  if (!camps) return;
  for (const zone of camps.zones) {
    const label = zone.names[0] || zone.id;
    const opt = document.createElement("option");
    opt.value = label;
    opt.textContent = `${label} (${formatRespawnSecs(zone.default_respawn_secs)})`;
    if (current && (label.toLowerCase() === current.toLowerCase() || zone.names.some((n) => n.toLowerCase() === current.toLowerCase()))) {
      opt.selected = true;
    }
    select.appendChild(opt);
  }
  // Keep a custom/log zone that isn't in camps.json selectable.
  if (current && ![...select.options].some((o) => o.value.toLowerCase() === current.toLowerCase())) {
    const opt = document.createElement("option");
    opt.value = current;
    opt.textContent = current;
    opt.selected = true;
    select.appendChild(opt);
  }
}

function syncRespawnZoneSelect() {
  const select = document.getElementById("respawn-zone") as HTMLSelectElement | null;
  if (!select || !config) return;
  fillZoneSelect(select, config.respawn_zone ?? "");
}

async function applyRespawnZone(zone: string) {
  if (!config) return;
  const payload = await invoke<{ timers: unknown[]; zone: string | null }>("set_respawn_zone", {
    zone,
  });
  config.respawn_zone = payload.zone ?? zone;
  syncRespawnZoneSelect();
}

function renderRareList() {
  const list = $("rare-list");
  list.innerHTML = "";
  if (!config || !camps) {
    list.innerHTML = `<div class="hint">Loading camps…</div>`;
    return;
  }
  const watched = config.watched_rares ?? {};
  const zonesWithRares = camps.zones.filter((z) => z.rares.length > 0);
  if (!zonesWithRares.length) {
    list.innerHTML = `<div class="hint">No rares defined yet — all kills use the zone default above.</div>`;
    return;
  }
  for (const zone of zonesWithRares) {
    const details = document.createElement("details");
    details.className = "class-section";
    details.open = true;
    const summary = document.createElement("summary");
    summary.className = "class-summary";
    const zoneName = zone.names[0] || zone.id;
    summary.innerHTML = `
      <span class="class-name">${escapeHtml(zoneName)}</span>
      <span class="class-count">${zone.rares.length} · default ${formatRespawnSecs(zone.default_respawn_secs)}</span>
    `;
    details.appendChild(summary);
    const body = document.createElement("div");
    body.className = "class-body";
    for (const rare of zone.rares) {
      const on = watched[rare.id] ?? rare.watched_by_default ?? true;
      const row = document.createElement("div");
      row.className = "spell-row";
      row.innerHTML = `
        <label class="spell-watch">
          <input type="checkbox" data-rare="${escapeHtml(rare.id)}" class="rare-toggle" ${on ? "checked" : ""} />
          <span class="spell-name">${escapeHtml(rare.label)}</span>
          <span class="badge buff">${escapeHtml(formatRespawnSecs(rare.respawn_secs))}</span>
        </label>
      `;
      body.appendChild(row);
    }
    details.appendChild(body);
    list.appendChild(details);
  }
  list.querySelectorAll<HTMLInputElement>(".rare-toggle").forEach((el) => {
    el.addEventListener("change", () => {
      const id = el.dataset.rare!;
      if (!config) return;
      if (!config.watched_rares) config.watched_rares = {};
      config.watched_rares[id] = el.checked;
      void persistAppearanceLive();
    });
  });
}

function resolveFontFamily(raw: string): string {
  const id = raw.trim() || DEFAULT_OVERLAY.font_family;
  const found = OVERLAY_FONTS.find((f) => f.id.toLowerCase() === id.toLowerCase());
  return found?.id ?? DEFAULT_OVERLAY.font_family;
}

function readAppearanceFromForm(): OverlayAppearance {
  const panelPct = Number((document.getElementById("ov-panel-opacity") as HTMLInputElement).value);
  const barPct = Number((document.getElementById("ov-bar-opacity") as HTMLInputElement).value);
  const size = (document.getElementById("ov-timer-size") as HTMLSelectElement).value || "normal";
  const theme = (document.getElementById("ov-theme") as HTMLSelectElement).value || "berry";
  const fontFamily = resolveFontFamily(
    (document.getElementById("ov-font-family") as HTMLSelectElement).value || DEFAULT_OVERLAY.font_family
  );
  const showIcons = (document.getElementById("ov-show-icons") as HTMLInputElement).checked;
  const rightClickDismiss = (document.getElementById("ov-right-click-dismiss") as HTMLInputElement)
    .checked;
  const showRecentlyWoreOff = (
    document.getElementById("ov-show-recently-wore-off") as HTMLInputElement
  ).checked;
  const recentlyWoreOffSecs = clampRecentlyWoreOffSecs(
    Number((document.getElementById("ov-recently-wore-off-secs") as HTMLInputElement).value)
  );
  const separateEnemyWindow = (
    document.getElementById("ov-separate-enemy-window") as HTMLInputElement
  ).checked;
  const selfBuffsOnly = (document.getElementById("ov-self-buffs-only") as HTMLInputElement)
    .checked;
  const hideOtherPets = (document.getElementById("ov-hide-other-pets") as HTMLInputElement)
    .checked;
  const voiceAnnouncements = (
    document.getElementById("ov-voice-announcements") as HTMLInputElement
  ).checked;
  const voiceUri = (document.getElementById("ov-voice-uri") as HTMLSelectElement).value.trim();
  const showRespawnWindow = (document.getElementById("ov-show-respawn-window") as HTMLInputElement)
    .checked;
  const trackAllKills = (document.getElementById("ov-track-all-kills") as HTMLInputElement)
    .checked;
  return {
    text_color: (document.getElementById("ov-text-color") as HTMLInputElement).value,
    panel_color: (document.getElementById("ov-panel-color") as HTMLInputElement).value,
    buff_color: (document.getElementById("ov-buff-color") as HTMLInputElement).value,
    debuff_color: (document.getElementById("ov-debuff-color") as HTMLInputElement).value,
    dot_color: (document.getElementById("ov-dot-color") as HTMLInputElement).value,
    panel_opacity: Math.min(1, Math.max(0, (Number.isFinite(panelPct) ? panelPct : 82) / 100)),
    bar_opacity: Math.min(1, Math.max(0, (Number.isFinite(barPct) ? barPct : 100) / 100)),
    timer_size: (TIMER_SIZES as readonly string[]).includes(size) ? size : "normal",
    theme,
    font_family: fontFamily,
    show_icons: showIcons,
    right_click_dismiss: rightClickDismiss,
    show_recently_wore_off: showRecentlyWoreOff,
    recently_wore_off_secs: recentlyWoreOffSecs,
    separate_enemy_window: separateEnemyWindow,
    self_buffs_only: selfBuffsOnly,
    hide_other_pets: hideOtherPets,
    voice_announcements: voiceAnnouncements,
    voice_uri: voiceUri,
    show_respawn_window: showRespawnWindow,
    track_all_kills: trackAllKills,
    // Native DWM border is tied to lock state; keep prior value for config compat.
    show_window_border: config?.overlay?.show_window_border ?? DEFAULT_OVERLAY.show_window_border,
  };
}

function writeAppearanceToForm(ov: OverlayAppearance) {
  const merged = { ...DEFAULT_OVERLAY, ...ov };
  (document.getElementById("ov-theme") as HTMLSelectElement).value = merged.theme || "berry";
  (document.getElementById("ov-text-color") as HTMLInputElement).value = merged.text_color;
  (document.getElementById("ov-panel-color") as HTMLInputElement).value = merged.panel_color;
  (document.getElementById("ov-buff-color") as HTMLInputElement).value = merged.buff_color;
  (document.getElementById("ov-debuff-color") as HTMLInputElement).value = merged.debuff_color;
  (document.getElementById("ov-dot-color") as HTMLInputElement).value = merged.dot_color;
  const panelPct = Math.round(merged.panel_opacity * 100);
  const barPct = Math.round(merged.bar_opacity * 100);
  (document.getElementById("ov-panel-opacity") as HTMLInputElement).value = String(panelPct);
  (document.getElementById("ov-bar-opacity") as HTMLInputElement).value = String(barPct);
  $("ov-panel-opacity-label").textContent = `${panelPct}%`;
  $("ov-bar-opacity-label").textContent = `${barPct}%`;
  (document.getElementById("ov-timer-size") as HTMLSelectElement).value = merged.timer_size || "normal";
  (document.getElementById("ov-font-family") as HTMLSelectElement).value = resolveFontFamily(
    merged.font_family || DEFAULT_OVERLAY.font_family
  );
  (document.getElementById("ov-show-icons") as HTMLInputElement).checked = merged.show_icons !== false;
  (document.getElementById("ov-right-click-dismiss") as HTMLInputElement).checked =
    merged.right_click_dismiss !== false;
  (document.getElementById("ov-show-recently-wore-off") as HTMLInputElement).checked =
    merged.show_recently_wore_off !== false;
  const recentSecs = clampRecentlyWoreOffSecs(merged.recently_wore_off_secs);
  (document.getElementById("ov-recently-wore-off-secs") as HTMLInputElement).value =
    String(recentSecs);
  $("ov-recently-wore-off-label").textContent = formatRecentlyWoreOffLabel(recentSecs);
  (document.getElementById("ov-separate-enemy-window") as HTMLInputElement).checked =
    !!merged.separate_enemy_window;
  (document.getElementById("ov-self-buffs-only") as HTMLInputElement).checked =
    !!merged.self_buffs_only;
  (document.getElementById("ov-hide-other-pets") as HTMLInputElement).checked =
    !!merged.hide_other_pets;
  (document.getElementById("ov-voice-announcements") as HTMLInputElement).checked =
    merged.voice_announcements !== false;
  populateVoiceSelect(merged.voice_uri ?? "");
  (document.getElementById("ov-show-respawn-window") as HTMLInputElement).checked =
    merged.show_respawn_window !== false;
  (document.getElementById("ov-track-all-kills") as HTMLInputElement).checked =
    merged.track_all_kills !== false;
}

/** Preferred voiceURI across async voiceschanged rebuilds (empty = system default). */
let voiceSelectPreferred = "";

/** Populate announcement-voice dropdown from Web Speech voices. */
function populateVoiceSelect(preferredUri?: string) {
  const select = document.getElementById("ov-voice-uri") as HTMLSelectElement | null;
  if (!select) return;
  if (preferredUri !== undefined) {
    voiceSelectPreferred = preferredUri.trim();
  }
  const preferred = voiceSelectPreferred;
  const voices =
    typeof speechSynthesis !== "undefined" ? speechSynthesis.getVoices() : [];
  select.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = "System default";
  select.appendChild(def);
  for (const v of voices) {
    const opt = document.createElement("option");
    opt.value = v.voiceURI;
    opt.textContent = `${v.name} (${v.lang})`;
    select.appendChild(opt);
  }
  // Missing saved voice → fall back to System default in the UI.
  if (preferred && [...select.options].some((o) => o.value === preferred)) {
    select.value = preferred;
  } else {
    select.value = "";
  }
}

function initVoiceSelect() {
  populateVoiceSelect(config?.overlay?.voice_uri ?? "");
  if (typeof speechSynthesis === "undefined") return;
  speechSynthesis.addEventListener("voiceschanged", () => {
    populateVoiceSelect();
  });
}

function testAnnouncementVoice() {
  if (typeof speechSynthesis === "undefined") return;
  const uri = (document.getElementById("ov-voice-uri") as HTMLSelectElement).value.trim();
  const voices = speechSynthesis.getVoices();
  const voice =
    (uri && (voices.find((v) => v.voiceURI === uri) ?? voices.find((v) => v.name === uri))) ||
    null;
  speechSynthesis.cancel();
  const utter = new SpeechSynthesisUtterance("Clarity has worn off");
  utter.rate = 1.05;
  if (voice) utter.voice = voice;
  speechSynthesis.speak(utter);
}

function readFormIntoConfig(): AppConfig {
  if (!config) throw new Error("Config not loaded");
  const next: AppConfig = {
    ...config,
    log_path: (document.getElementById("log-path") as HTMLInputElement).value,
    character_level: Number((document.getElementById("char-level") as HTMLInputElement).value) || 1,
    my_pet_name: (document.getElementById("my-pet-name") as HTMLInputElement).value.trim(),
    overlay_locked: (document.getElementById("overlay-locked") as HTMLInputElement).checked,
    spell_tiers: { ...config.spell_tiers },
    watched: { ...config.watched },
    watched_rares: { ...(config.watched_rares ?? {}) },
    camp_overrides: { ...(config.camp_overrides ?? {}) },
    respawn_zone: (document.getElementById("respawn-zone") as HTMLSelectElement).value,
    loot_tracking: (document.getElementById("loot-tracking") as HTMLInputElement).checked,
    loot_sync_enabled: (document.getElementById("loot-sync-enabled") as HTMLInputElement).checked,
    loot_sync_url: (document.getElementById("loot-sync-url") as HTMLInputElement).value.trim(),
    loot_sync_key: (document.getElementById("loot-sync-key") as HTMLInputElement).value.trim(),
    overlay: readAppearanceFromForm(),
  };
  const seen = new Set<string>();
  document.querySelectorAll<HTMLInputElement>(".watch-toggle").forEach((el) => {
    const name = el.dataset.spell!;
    if (seen.has(name)) return;
    seen.add(name);
    next.watched[name] = el.checked;
  });
  document.querySelectorAll<HTMLInputElement>(".rare-toggle").forEach((el) => {
    const id = el.dataset.rare!;
    next.watched_rares![id] = el.checked;
  });
  return next;
}

async function persistAppearanceLive() {
  if (!config) return;
  const next = readFormIntoConfig();
  config = await invoke<AppConfig>("save_settings", { config: next });
}

function scheduleAppearanceSave() {
  $("ov-panel-opacity-label").textContent = `${(document.getElementById("ov-panel-opacity") as HTMLInputElement).value}%`;
  $("ov-bar-opacity-label").textContent = `${(document.getElementById("ov-bar-opacity") as HTMLInputElement).value}%`;
  const recentSecs = clampRecentlyWoreOffSecs(
    Number((document.getElementById("ov-recently-wore-off-secs") as HTMLInputElement).value)
  );
  $("ov-recently-wore-off-label").textContent = formatRecentlyWoreOffLabel(recentSecs);
  if (appearanceSaveTimer) clearTimeout(appearanceSaveTimer);
  appearanceSaveTimer = setTimeout(() => {
    void persistAppearanceLive();
  }, 200);
}

function applyThemeFromSelect() {
  if (!config) return;
  const themeId = (document.getElementById("ov-theme") as HTMLSelectElement).value;
  const current = readAppearanceFromForm();
  const next = applyThemePreset(current, themeId);
  writeAppearanceToForm(next);
  scheduleAppearanceSave();
}

function renderLiveTimers(timers: ActiveTimer[]) {
  const box = $("live-timers");
  if (!timers.length) {
    box.className = "live-timers empty";
    box.textContent = "No active timers";
    return;
  }
  box.className = "live-timers";
  const sorted = [...timers].sort(
    (a, b) => new Date(a.ends_at).getTime() - new Date(b.ends_at).getTime(),
  );
  box.innerHTML = sorted
    .map((t) => {
      const { text } = formatRemain(t.ends_at);
      const total = t.duration_secs * 1000;
      const left = Math.max(0, new Date(t.ends_at).getTime() - Date.now());
      const pct = total > 0 ? (left / total) * 100 : 0;
      const urgent = left > 0 && left < 30_000;
      return `<div class="timer-row cat-${t.category}${urgent ? " timer-urgent" : ""}">
        <div class="timer-meta">${iconImgHtml(spellIconByName(t.spell))} <strong>${escapeHtml(t.spell)}</strong> — ${escapeHtml(t.target)}</div>
        <div class="timer-time">${text}</div>
        <div class="bar"><div class="bar-fill" style="width:${pct}%"></div></div>
      </div>`;
    })
    .join("");
}

interface SuggestedLog {
  path: string;
  label: string;
}

async function loadLogSuggestions() {
  const wrap = document.getElementById("log-suggest-wrap");
  const empty = document.getElementById("log-suggest-empty");
  const select = document.getElementById("log-suggest") as HTMLSelectElement | null;
  if (!wrap || !empty || !select) return;

  let suggestions: SuggestedLog[] = [];
  try {
    suggestions = await invoke<SuggestedLog[]>("suggest_log_paths");
  } catch {
    suggestions = [];
  }

  select.innerHTML = "";
  if (suggestions.length === 0) {
    wrap.hidden = true;
    empty.hidden = !!(config && !config.log_path);
    return;
  }

  empty.hidden = true;
  wrap.hidden = false;
  for (const item of suggestions) {
    const opt = document.createElement("option");
    opt.value = item.path;
    opt.textContent = item.label;
    opt.title = item.path;
    select.appendChild(opt);
  }

  const currentLog = config?.log_path ?? "";
  if (currentLog) {
    const match = suggestions.find((s) => s.path === currentLog);
    if (match) select.value = match.path;
  } else {
    // First run: prefill the newest detected log (still requires Save Settings).
    select.value = suggestions[0].path;
    (document.getElementById("log-path") as HTMLInputElement).value = suggestions[0].path;
  }
}

async function load() {
  spells = await invoke<SpellDef[]>("get_spells");
  camps = await invoke<CampsFile>("get_camps");
  config = await invoke<AppConfig>("get_config");
  (document.getElementById("log-path") as HTMLInputElement).value = config.log_path;
  (document.getElementById("char-level") as HTMLInputElement).value = String(config.character_level);
  (document.getElementById("my-pet-name") as HTMLInputElement).value = config.my_pet_name ?? "";
  (document.getElementById("my-pet-name") as HTMLInputElement).title = PET_NAME_HINT;
  (document.getElementById("overlay-locked") as HTMLInputElement).checked = config.overlay_locked;
  (document.getElementById("loot-tracking") as HTMLInputElement).checked =
    config.loot_tracking !== false;
  (document.getElementById("loot-sync-enabled") as HTMLInputElement).checked =
    !!config.loot_sync_enabled;
  (document.getElementById("loot-sync-url") as HTMLInputElement).value =
    config.loot_sync_url?.trim() || "https://norrathroster.com";
  (document.getElementById("loot-sync-key") as HTMLInputElement).value =
    config.loot_sync_key ?? "";
  writeAppearanceToForm(overlayOf(config));
  await loadLogSuggestions();
  renderSpellList();
  syncRespawnZoneSelect();
  renderRareList();
  const payload = await invoke<{ timers: ActiveTimer[] }>("get_timers");
  renderLiveTimers(payload.timers);
  await refreshLoot();
}

function formatRate(rate: number | null): string {
  if (rate == null || !Number.isFinite(rate)) return "—";
  return `${(rate * 100).toFixed(rate >= 0.1 ? 1 : 2)}%`;
}

function formatCopper(copper: number): string {
  if (!copper) return "—";
  const p = Math.floor(copper / 1000);
  const g = Math.floor((copper % 1000) / 100);
  const s = Math.floor((copper % 100) / 10);
  const c = copper % 10;
  const parts: string[] = [];
  if (p) parts.push(`${p}p`);
  if (g) parts.push(`${g}g`);
  if (s) parts.push(`${s}s`);
  if (c || parts.length === 0) parts.push(`${c}c`);
  return parts.join(" ");
}

function renderLootSummary(snap: LootSnapshot) {
  const el = document.getElementById("loot-summary");
  if (!el) return;
  const zone = snap.zone ? ` · zone ${escapeHtml(snap.zone)}` : "";
  el.innerHTML = `
    <span><strong>${snap.total_kills}</strong> kills</span>
    <span><strong>${snap.total_item_appearances}</strong> item drops</span>
    <span><strong>${snap.mob_count}</strong> mobs</span>
    <span class="hint" style="margin:0">${zone || "· zone unknown"}</span>
  `;
}

function renderLootTable(snap: LootSnapshot) {
  const wrap = document.getElementById("loot-table");
  if (!wrap) return;
  renderLootSummary(snap);

  if (lootView === "mobs") {
    if (!snap.mobs.length) {
      wrap.innerHTML = `<div class="hint">No mobs match — kill and loot while tracking is on.</div>`;
      return;
    }
    wrap.innerHTML = `<table class="loot-table">
      <thead>
        <tr>
          <th>Mob</th>
          <th>Zone</th>
          <th class="num">Kills</th>
          <th class="num">Items</th>
          <th class="num">Avg coin</th>
        </tr>
      </thead>
      <tbody>
        ${snap.mobs
          .map(
            (m) => `<tr>
          <td>${escapeHtml(m.mob)}</td>
          <td>${escapeHtml(m.zone)}</td>
          <td class="num">${m.kills}</td>
          <td class="num">${m.unique_items}</td>
          <td class="num">${escapeHtml(formatCopper(m.coin_avg_copper))}</td>
        </tr>`
          )
          .join("")}
      </tbody>
    </table>`;
    return;
  }

  if (!snap.items.length) {
    wrap.innerHTML = `<div class="hint">No items match — loot corpses while tracking is on.</div>`;
    return;
  }
  wrap.innerHTML = `<table class="loot-table">
    <thead>
      <tr>
        <th>Item</th>
        <th>Mob</th>
        <th>Zone</th>
        <th class="num">Times</th>
        <th class="num">Qty</th>
        <th class="num">Kills</th>
        <th class="num">Rate</th>
      </tr>
    </thead>
    <tbody>
      ${snap.items
        .map(
          (i) => `<tr>
        <td>${escapeHtml(i.item)}</td>
        <td>${escapeHtml(i.mob)}</td>
        <td>${escapeHtml(i.zone)}</td>
        <td class="num">${i.times}</td>
        <td class="num">${i.quantity}</td>
        <td class="num">${i.kills}</td>
        <td class="num loot-rate">${formatRate(i.rate)}</td>
      </tr>`
        )
        .join("")}
    </tbody>
  </table>`;
}

async function refreshLoot(query = lootSearch) {
  lootSnapshot = await invoke<LootSnapshot>("get_loot", { query });
  renderLootTable(lootSnapshot);
}

function scheduleLootRefresh() {
  if (lootSearchTimer) clearTimeout(lootSearchTimer);
  lootSearchTimer = setTimeout(() => {
    void refreshLoot(lootSearch);
  }, 180);
}

function initSectionNav() {
  const tabs = document.querySelectorAll<HTMLButtonElement>(".section-tab");
  const panels = document.querySelectorAll<HTMLElement>("[data-section-panel]");
  const activate = (id: string) => {
    for (const tab of tabs) {
      const on = tab.dataset.section === id;
      tab.classList.toggle("is-active", on);
      tab.setAttribute("aria-selected", on ? "true" : "false");
    }
    for (const panel of panels) {
      const on = panel.dataset.sectionPanel === id;
      panel.classList.toggle("is-active", on);
      panel.hidden = !on;
    }
    if (id === "loot") {
      void refreshLoot();
    }
  };
  for (const tab of tabs) {
    tab.addEventListener("click", () => {
      const id = tab.dataset.section;
      if (id) activate(id);
    });
  }
}

function initLootUi() {
  const search = document.getElementById("loot-search") as HTMLInputElement | null;
  if (search) {
    search.addEventListener("input", () => {
      lootSearch = search.value;
      scheduleLootRefresh();
    });
  }
  document.querySelectorAll<HTMLButtonElement>("[data-loot-view]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const view = btn.dataset.lootView;
      if (view !== "mobs" && view !== "items") return;
      lootView = view;
      document.querySelectorAll<HTMLButtonElement>("[data-loot-view]").forEach((b) => {
        b.classList.toggle("is-active", b.dataset.lootView === view);
      });
      if (lootSnapshot) renderLootTable(lootSnapshot);
      else void refreshLoot();
    });
  });
  $("btn-clear-loot").addEventListener("click", async () => {
    if (!confirm("Clear all local loot tracking data?")) return;
    await invoke("clear_loot");
    await refreshLoot();
    const status = $("loot-status");
    status.textContent = "Cleared";
    setTimeout(() => (status.textContent = ""), 2000);
  });
  $("btn-upload-loot").addEventListener("click", async () => {
    const status = $("loot-status");
    status.textContent = "Uploading…";
    try {
      // Persist sync settings first so the Rust side sees the latest key/URL.
      const next = readFormIntoConfig();
      config = await invoke<AppConfig>("save_settings", { config: next });
      const result = await invoke<{
        ok: boolean;
        message: string;
        kills_added: number | null;
        drops_added: number | null;
      }>("upload_loot");
      status.textContent = result.ok
        ? `${result.message}${
            result.kills_added != null || result.drops_added != null
              ? ` (+${result.kills_added ?? 0} kills, +${result.drops_added ?? 0} drops)`
              : ""
          }`
        : result.message;
    } catch (err) {
      status.textContent = err instanceof Error ? err.message : String(err);
    }
    setTimeout(() => {
      if (status.textContent?.startsWith("Uploaded") || status.textContent === "Cleared") {
        status.textContent = "";
      }
    }, 5000);
  });
  $("btn-save-loot").addEventListener("click", async () => {
    const next = readFormIntoConfig();
    config = await invoke<AppConfig>("save_settings", { config: next });
    const status = $("loot-status");
    status.textContent = "Saved";
    setTimeout(() => (status.textContent = ""), 2000);
  });
  $("loot-tracking").addEventListener("change", async () => {
    if (!config) return;
    const next = readFormIntoConfig();
    config = await invoke<AppConfig>("save_settings", { config: next });
  });
}

function initSettingsTabs() {
  const tabs = document.querySelectorAll<HTMLButtonElement>(".settings-tab:not([data-loot-view])");
  const panels = document.querySelectorAll<HTMLElement>("[data-tab-panel]");
  const activate = (id: string) => {
    for (const tab of tabs) {
      const on = tab.dataset.tab === id;
      tab.classList.toggle("is-active", on);
      tab.setAttribute("aria-selected", on ? "true" : "false");
    }
    for (const panel of panels) {
      const on = panel.dataset.tabPanel === id;
      panel.classList.toggle("is-active", on);
      panel.hidden = !on;
    }
  };
  for (const tab of tabs) {
    tab.addEventListener("click", () => {
      const id = tab.dataset.tab;
      if (id) activate(id);
    });
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  initSectionNav();
  initSettingsTabs();
  initLootUi();
  await load();
  initVoiceSelect();

  $("btn-browse").addEventListener("click", async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "EQ Log", extensions: ["txt", "log"] }],
    });
    if (typeof selected === "string") {
      (document.getElementById("log-path") as HTMLInputElement).value = selected;
    }
  });

  const btnUseLog = document.getElementById("btn-use-log");
  if (btnUseLog) {
    btnUseLog.addEventListener("click", () => {
      const select = document.getElementById("log-suggest") as HTMLSelectElement | null;
      if (select?.value) {
        (document.getElementById("log-path") as HTMLInputElement).value = select.value;
      }
    });
  }

  $("btn-save").addEventListener("click", async () => {
    const next = readFormIntoConfig();
    config = await invoke<AppConfig>("save_settings", { config: next });
    await invoke("set_overlay_locked", { locked: config.overlay_locked });
    const status = $("status");
    status.textContent = "Saved";
    setTimeout(() => (status.textContent = ""), 2000);
  });

  $("btn-clear").addEventListener("click", async () => {
    await invoke("clear_timers");
  });

  $("btn-clear-respawns").addEventListener("click", async () => {
    await invoke("clear_respawns");
  });

  $("respawn-zone").addEventListener("change", async (e) => {
    const zone = (e.target as HTMLSelectElement).value;
    await applyRespawnZone(zone);
  });

  $("btn-show-overlay").addEventListener("click", async () => {
    await invoke("show_overlay");
  });

  $("overlay-locked").addEventListener("change", async (e) => {
    const locked = (e.target as HTMLInputElement).checked;
    await invoke("set_overlay_locked", { locked });
  });

  $("spell-search").addEventListener("input", (e) => {
    spellSearch = (e.target as HTMLInputElement).value;
    renderSpellList();
  });

  const appearanceIds = [
    "ov-text-color",
    "ov-panel-color",
    "ov-buff-color",
    "ov-debuff-color",
    "ov-dot-color",
    "ov-panel-opacity",
    "ov-bar-opacity",
    "ov-timer-size",
    "ov-font-family",
    "ov-show-icons",
    "ov-right-click-dismiss",
    "ov-show-recently-wore-off",
    "ov-recently-wore-off-secs",
    "ov-separate-enemy-window",
    "ov-self-buffs-only",
    "ov-hide-other-pets",
    "ov-voice-announcements",
    "ov-voice-uri",
    "my-pet-name",
    "ov-show-respawn-window",
    "ov-track-all-kills",
  ];
  for (const id of appearanceIds) {
    $(id).addEventListener("input", scheduleAppearanceSave);
    $(id).addEventListener("change", scheduleAppearanceSave);
  }

  $("ov-theme").addEventListener("change", applyThemeFromSelect);

  $("ov-voice-uri").addEventListener("change", () => {
    voiceSelectPreferred = (
      document.getElementById("ov-voice-uri") as HTMLSelectElement
    ).value.trim();
  });

  $("btn-test-voice").addEventListener("click", () => {
    testAnnouncementVoice();
  });

  $("btn-reset-appearance").addEventListener("click", () => {
    writeAppearanceToForm(DEFAULT_OVERLAY);
    scheduleAppearanceSave();
  });

  await listen<{ timers: ActiveTimer[]; recent_expired?: unknown[] }>("timers-updated", (event) => {
    renderLiveTimers(event.payload.timers);
  });

  await listen<LootSnapshot>("loot-updated", (event) => {
    // Keep filtered view in sync without wiping the search box.
    if (!lootSearch.trim()) {
      lootSnapshot = event.payload;
      renderLootTable(lootSnapshot);
    } else {
      void refreshLoot();
    }
  });

  await listen<AppConfig>("config-updated", (event) => {
    config = event.payload;
    (document.getElementById("my-pet-name") as HTMLInputElement).value =
      config.my_pet_name ?? "";
    (document.getElementById("loot-tracking") as HTMLInputElement).checked =
      config.loot_tracking !== false;
    (document.getElementById("loot-sync-enabled") as HTMLInputElement).checked =
      !!config.loot_sync_enabled;
    (document.getElementById("loot-sync-url") as HTMLInputElement).value =
      config.loot_sync_url?.trim() || "https://norrathroster.com";
    if (!(document.getElementById("loot-sync-key") as HTMLInputElement).value) {
      (document.getElementById("loot-sync-key") as HTMLInputElement).value =
        config.loot_sync_key ?? "";
    }
    writeAppearanceToForm(overlayOf(config));
    syncRespawnZoneSelect();
  });
});
