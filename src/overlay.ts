import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  DEFAULT_OVERLAY,
  TIMER_SIZES,
  iconImgHtml,
  overlayFontCss,
  type OverlayAppearance,
} from "./themes";
import { isMyPet, isPetTarget, keepFriendlyTarget } from "./pets";
interface ActiveTimer {
  id: string;
  spell: string;
  target: string;
  category: string;
  started_at: string;
  ends_at: string;
  duration_secs: number;
}
interface RecentExpired {
  id: string;
  spell: string;
  target: string;
  category: string;
  ended_at: string;
}
interface TimersPayload {
  timers: ActiveTimer[];
  recent_expired: RecentExpired[];
}
interface SpellDef {
  name: string;
  spellicon?: string;
}
interface AppConfig {
  overlay_locked: boolean;
  respawn_zone?: string;
  my_pet_name?: string;
  overlay?: OverlayAppearance;
}

interface ZoneCamps {
  id: string;
  names: string[];
  default_respawn_secs: number;
}

interface CampsFile {
  zones: ZoneCamps[];
}

interface RespawnTimer {
  id: string;
  zone_id: string;
  zone_name: string;
  label: string;
  npc_name: string;
  rare_id: string | null;
  is_rare: boolean;
  started_at: string;
  ends_at: string;
  duration_secs: number;
}

interface RespawnsPayload {
  timers: RespawnTimer[];
  zone: string | null;
}

/** Window role: main, enemies-only, or respawns. */
type OverlayRole = "main" | "enemies" | "respawns";

let locked = false;
let appearance: OverlayAppearance = { ...DEFAULT_OVERLAY };
let iconBySpell = new Map<string, string>();
let rightClickDismiss = true;
let showRecentlyWoreOff = true;
let separateEnemyWindow = false;
let selfBuffsOnly = false;
let hideOtherPets = false;
let voiceAnnouncements = true;
/** Web Speech voiceURI; empty = system default. */
let voiceUri = "";
/** TTS volume 0–1. */
let voiceVolume = 1;
let myPetName = "";
let overlayRole: OverlayRole = "main";
let lastTimers: ActiveTimer[] = [];
let lastRecent: RecentExpired[] = [];
let lastRespawns: RespawnTimer[] = [];
let lastRespawnZone: string | null = null;
let campsCache: CampsFile | null = null;
/** Recent-expired IDs already announced (or present at startup). Main overlay only. */
let announcedRecentIds = new Set<string>();
let voicePrimed = false;
function hexToRgb(hex: string): { r: number; g: number; b: number } | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}
function withAlpha(hex: string, alpha: number): string {
  const rgb = hexToRgb(hex);
  if (!rgb) return hex;
  const a = Math.min(1, Math.max(0, alpha));
  return `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, ${a})`;
}
/** Faction-drop / pacify line (often `buff` in spells.json; EQ Beneficial). */
function isLullSpell(spell: string): boolean {
  const n = spell.toLowerCase();
  if (n.includes("wake of tranquility") || n.includes("evanescence")) return true;
  return ["calm", "soothe", "lull", "pacify", "harmony"].some((t) => n.includes(t));
}
/** Enemy / detrimental: debuff, DoT, or lull/pacify on someone other than You. */
function isEnemyTimer(category: string, target: string, spell: string): boolean {
  if (target.toLowerCase() === "you") return false;
  const cat = category.toLowerCase();
  return cat === "debuff" || cat === "dot" || cat === "lull" || isLullSpell(spell);
}
function isFriendlyTimer(category: string, target: string, spell: string): boolean {
  return !isEnemyTimer(category, target, spell);
}
function applyConfig(cfg: AppConfig) {
  applyAppearance(cfg.overlay ?? DEFAULT_OVERLAY);
  myPetName = (cfg.my_pet_name ?? "").trim();
}

function applyAppearance(ov: OverlayAppearance) {
  appearance = { ...DEFAULT_OVERLAY, ...ov };
  rightClickDismiss = appearance.right_click_dismiss !== false;
  showRecentlyWoreOff = appearance.show_recently_wore_off !== false;
  separateEnemyWindow = !!appearance.separate_enemy_window;
  selfBuffsOnly = !!appearance.self_buffs_only;
  hideOtherPets = !!appearance.hide_other_pets;
  voiceAnnouncements = appearance.voice_announcements !== false;
  voiceUri = (appearance.voice_uri ?? "").trim();
  const vol = Number(appearance.voice_volume);
  voiceVolume = Number.isFinite(vol) ? Math.min(1, Math.max(0, vol)) : 1;
  const root = document.documentElement;
  root.style.setProperty("--ov-text", appearance.text_color);
  root.style.setProperty("--ov-panel", withAlpha(appearance.panel_color, appearance.panel_opacity));
  root.style.setProperty("--ov-buff", withAlpha(appearance.buff_color, appearance.bar_opacity));
  root.style.setProperty("--ov-debuff", withAlpha(appearance.debuff_color, appearance.bar_opacity));
  root.style.setProperty("--ov-dot", withAlpha(appearance.dot_color, appearance.bar_opacity));
  root.style.setProperty("--ov-muted", withAlpha(appearance.text_color, 0.7));
  root.style.setProperty("--ov-faint", withAlpha(appearance.text_color, 0.55));
  root.style.setProperty("--overlay-font", overlayFontCss(appearance.font_family));
  const size = (TIMER_SIZES as readonly string[]).includes(appearance.timer_size)
    ? appearance.timer_size
    : "normal";
  document.body.dataset.size = size;
  document.body.dataset.theme = appearance.theme || "berry";
  document.body.dataset.role = overlayRole;
  document.body.classList.toggle("panel-clear", appearance.panel_opacity <= 0.01);
  document.body.classList.toggle("show-icons", appearance.show_icons !== false);
  document.body.classList.toggle("is-enemies", overlayRole === "enemies");
  document.body.classList.toggle("is-respawns", overlayRole === "respawns");
  const roleLabel = document.getElementById("role-label");
  if (roleLabel) {
    if (overlayRole === "enemies") {
      roleLabel.textContent = "Enemies";
      roleLabel.hidden = false;
    } else if (overlayRole === "respawns") {
      roleLabel.textContent = "Respawns";
      roleLabel.hidden = false;
    } else {
      roleLabel.textContent = "";
      roleLabel.hidden = true;
    }
  }
  const zoneSelect = document.getElementById("zone-select") as HTMLSelectElement | null;
  if (zoneSelect) {
    zoneSelect.hidden = overlayRole !== "respawns";
  }
}
function formatRemain(endsAt: string): string {
  const secs = Math.max(0, Math.ceil((new Date(endsAt).getTime() - Date.now()) / 1000));
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
/** Relative age for recently wore-off rows, e.g. "12s ago" / "3m ago". */
function formatAgo(endedAt: string): string {
  const secs = Math.max(0, Math.floor((Date.now() - new Date(endedAt).getTime()) / 1000));
  if (secs < 60) return `${secs}s ago`;
  const m = Math.floor(secs / 60);
  return `${m}m ago`;
}
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
function spellIconHtml(spellName: string): string {
  if (appearance.show_icons === false) return "";
  return iconImgHtml(iconBySpell.get(spellName), "oicon");
}
function filterForRole<T extends { category: string; target: string; spell: string }>(
  items: T[],
): T[] {
  let filtered: T[];
  if (!separateEnemyWindow) {
    filtered = items;
  } else if (overlayRole === "enemies") {
    filtered = items.filter((t) => isEnemyTimer(t.category, t.target, t.spell));
  } else {
    filtered = items.filter((t) => isFriendlyTimer(t.category, t.target, t.spell));
  }
  // Main overlay only: self-buffs / hide-other-pets. Enemies overlay unchanged.
  // Combined mode still keeps enemy timers on the single window.
  if (overlayRole === "main" && (selfBuffsOnly || hideOtherPets)) {
    filtered = filtered.filter((t) => {
      if (!separateEnemyWindow && isEnemyTimer(t.category, t.target, t.spell)) return true;
      return keepFriendlyTarget(t.target, {
        selfBuffsOnly,
        hideOtherPets,
        myPetName,
      });
    });
  }
  return filtered;
}

function formatTargetLabel(target: string): string {
  const pet = isPetTarget(target) || isMyPet(target, myPetName);
  if (!pet) return escapeHtml(target);
  return `${escapeHtml(target)} <span class="opet">pet</span>`;
}
/** You first; other targets by soonest remaining timer in the group. */
function compareTargetGroups<T extends { target: string }>(
  a: { target: string; items: T[] },
  b: { target: string; items: T[] },
  soonestMs: (items: T[]) => number
): number {
  const aYou = a.target.toLowerCase() === "you";
  const bYou = b.target.toLowerCase() === "you";
  if (aYou && !bYou) return -1;
  if (!aYou && bYou) return 1;
  const bySoonest = soonestMs(a.items) - soonestMs(b.items);
  if (bySoonest !== 0) return bySoonest;
  return a.target.localeCompare(b.target, undefined, { sensitivity: "base" });
}
function groupByTarget<T extends { target: string }>(
  items: T[],
  sortWithin: (a: T, b: T) => number,
  compareGroups?: (
    a: { target: string; items: T[] },
    b: { target: string; items: T[] }
  ) => number
): { target: string; items: T[] }[] {
  const map = new Map<string, { target: string; items: T[] }>();
  for (const item of items) {
    const key = item.target.toLowerCase();
    const existing = map.get(key);
    if (existing) {
      existing.items.push(item);
    } else {
      map.set(key, { target: item.target || "Unknown", items: [item] });
    }
  }
  const groups = Array.from(map.values());
  for (const g of groups) {
    g.items.sort(sortWithin);
  }
  // Caller supplies group order via optional compareGroups; default You-first alpha.
  groups.sort((a, b) => {
    if (compareGroups) return compareGroups(a, b);
    return compareTargetGroups(a, b, () => 0);
  });
  return groups;
}
function remainMs(endsAt: string): number {
  return Math.max(0, new Date(endsAt).getTime() - Date.now());
}
const URGENT_REMAIN_SECS = 30;
function renderActiveRow(t: ActiveTimer, hideTarget: boolean): string {
  const total = t.duration_secs * 1000;
  const left = remainMs(t.ends_at);
  const pct = total > 0 ? (left / total) * 100 : 0;
  const urgent = left > 0 && left < URGENT_REMAIN_SECS * 1000;
  const urgentClass = urgent ? " otimer-urgent" : "";
  const icon = spellIconHtml(t.spell);
  const minimal = appearance.timer_size === "minimal";
  if (minimal) {
    const targetBit =
      !hideTarget && t.target
        ? ` <span class="otarget-inline">${escapeHtml(t.target)}</span>`
        : "";
    return `<div class="otimer cat-${escapeHtml(t.category)}${urgentClass}" data-timer-id="${escapeHtml(t.id)}">
      <div class="otop compact">
        ${icon}
        <span class="ospell">${escapeHtml(t.spell)}${targetBit}</span>
        <span class="otime">${formatRemain(t.ends_at)}</span>
      </div>
      <div class="obar"><div class="ofill" style="width:${pct}%"></div></div>
    </div>`;
  }
  const targetRow =
    hideTarget || !t.target ? "" : `<div class="otarget">${escapeHtml(t.target)}</div>`;
  return `<div class="otimer cat-${escapeHtml(t.category)}${urgentClass}" data-timer-id="${escapeHtml(t.id)}">
    <div class="otop">
      ${icon}
      <span class="ospell">${escapeHtml(t.spell)}</span>
      <span class="otime">${formatRemain(t.ends_at)}</span>
    </div>
    ${targetRow}
    <div class="obar"><div class="ofill" style="width:${pct}%"></div></div>
  </div>`;
}
function renderRecentRow(r: RecentExpired, hideTarget: boolean): string {
  const icon = spellIconHtml(r.spell);
  const minimal = appearance.timer_size === "minimal";
  if (minimal) {
    const targetBit =
      !hideTarget && r.target
        ? ` <span class="otarget-inline">${escapeHtml(r.target)}</span>`
        : "";
    return `<div class="otimer otimer-recent cat-${escapeHtml(r.category)}" data-recent-id="${escapeHtml(r.id)}">
      <div class="otop compact">
        ${icon}
        <span class="ospell">${escapeHtml(r.spell)}${targetBit}</span>
        <span class="otime otime-ago">${formatAgo(r.ended_at)}</span>
      </div>
    </div>`;
  }
  const targetRow =
    hideTarget || !r.target ? "" : `<div class="otarget">${escapeHtml(r.target)}</div>`;
  return `<div class="otimer otimer-recent cat-${escapeHtml(r.category)}" data-recent-id="${escapeHtml(r.id)}">
    <div class="otop">
      ${icon}
      <span class="ospell">${escapeHtml(r.spell)}</span>
      <span class="otime otime-ago">${formatAgo(r.ended_at)}</span>
    </div>
    ${targetRow}
  </div>`;
}
function renderGroupedActive(timers: ActiveTimer[]): string {
  const groups = groupByTarget(
    timers,
    (a, b) => remainMs(a.ends_at) - remainMs(b.ends_at),
    (a, b) =>
      compareTargetGroups(a, b, (items) =>
        Math.min(...items.map((t) => remainMs(t.ends_at)))
      )
  );
  return groups
    .map((g) => {
      const rows = g.items.map((t) => renderActiveRow(t, true)).join("");
      return `<div class="ogroup">
        <div class="ogroup-label">${formatTargetLabel(g.target)}</div>
        ${rows}
      </div>`;
    })
    .join("");
}
function renderGroupedRecent(recent: RecentExpired[]): string {
  const groups = groupByTarget(
    recent,
    (a, b) => new Date(b.ended_at).getTime() - new Date(a.ended_at).getTime()
  );
  return groups
    .map((g) => {
      const rows = g.items.map((r) => renderRecentRow(r, true)).join("");
      return `<div class="ogroup ogroup-recent">
        <div class="ogroup-label">${formatTargetLabel(g.target)}</div>
        ${rows}
      </div>`;
    })
    .join("");
}
function renderRespawnRow(t: RespawnTimer): string {
  const total = t.duration_secs * 1000;
  const left = remainMs(t.ends_at);
  const pct = total > 0 ? (left / total) * 100 : 0;
  const urgent = left > 0 && left < URGENT_REMAIN_SECS * 1000;
  const urgentClass = urgent ? " otimer-urgent" : "";
  const rareClass = t.is_rare ? " otimer-rare" : "";
  const cat = t.is_rare ? "rare" : "trash";
  const minimal = appearance.timer_size === "minimal";
  if (minimal) {
    return `<div class="otimer cat-${cat}${urgentClass}${rareClass}" data-timer-id="${escapeHtml(t.id)}">
      <div class="otop compact">
        <span class="ospell">${escapeHtml(t.label)}</span>
        <span class="otime">${formatRemain(t.ends_at)}</span>
      </div>
      <div class="obar"><div class="ofill" style="width:${pct}%"></div></div>
    </div>`;
  }
  const sub =
    t.is_rare && t.npc_name && t.npc_name !== t.label
      ? `<div class="otarget">${escapeHtml(t.npc_name)}</div>`
      : "";
  return `<div class="otimer cat-${cat}${urgentClass}${rareClass}" data-timer-id="${escapeHtml(t.id)}">
    <div class="otop">
      <span class="ospell">${escapeHtml(t.label)}</span>
      <span class="otime">${formatRemain(t.ends_at)}</span>
    </div>
    ${sub}
    <div class="obar"><div class="ofill" style="width:${pct}%"></div></div>
  </div>`;
}

function syncOverlayZoneSelect(zone: string | null) {
  const select = document.getElementById("zone-select") as HTMLSelectElement | null;
  if (!select || overlayRole !== "respawns") return;
  select.hidden = false;
  const current = (zone ?? "").trim();
  const prev = select.value;
  select.innerHTML = `<option value="">Select zone…</option>`;
  if (campsCache) {
    for (const z of campsCache.zones) {
      const label = z.names[0] || z.id;
      const opt = document.createElement("option");
      opt.value = label;
      opt.textContent = label;
      select.appendChild(opt);
    }
  }
  if (current && ![...select.options].some((o) => o.value.toLowerCase() === current.toLowerCase())) {
    const opt = document.createElement("option");
    opt.value = current;
    opt.textContent = current;
    select.appendChild(opt);
  }
  const match = [...select.options].find((o) => o.value.toLowerCase() === current.toLowerCase());
  select.value = match?.value ?? "";
  // Avoid fighting user mid-open if unchanged rebuild
  if (prev && select.value === prev) return;
}

/** Warm Web Speech voices (Windows often loads them async). */
function primeSpeech() {
  if (voicePrimed || typeof speechSynthesis === "undefined") return;
  voicePrimed = true;
  speechSynthesis.getVoices();
  speechSynthesis.addEventListener("voiceschanged", () => {
    speechSynthesis.getVoices();
  });
}

/** Resolve configured voice; missing/empty → null (system default). */
function resolveSpeechVoice(uri: string): SpeechSynthesisVoice | null {
  if (!uri || typeof speechSynthesis === "undefined") return null;
  const voices = speechSynthesis.getVoices();
  return (
    voices.find((v) => v.voiceURI === uri) ??
    voices.find((v) => v.name === uri) ??
    null
  );
}

function speakAnnouncement(text: string) {
  if (overlayRole !== "main" || !voiceAnnouncements) return;
  if (typeof speechSynthesis === "undefined") return;
  primeSpeech();
  const utter = new SpeechSynthesisUtterance(text);
  utter.rate = 1.05;
  utter.volume = voiceVolume;
  const voice = resolveSpeechVoice(voiceUri);
  if (voice) utter.voice = voice;
  speechSynthesis.speak(utter);
}

/** Announce newly worn-off renew buffs (main overlay only). */
function announceNewWornOff(recent: RecentExpired[]) {
  if (overlayRole !== "main") return;
  for (const r of recent) {
    if (announcedRecentIds.has(r.id)) continue;
    announcedRecentIds.add(r.id);
    speakAnnouncement(`${r.spell} has worn off`);
  }
  // Drop IDs that are no longer in the recent list so a later re-expire can speak again.
  const live = new Set(recent.map((r) => r.id));
  for (const id of [...announcedRecentIds]) {
    if (!live.has(id)) announcedRecentIds.delete(id);
  }
}

function seedAnnouncedRecent(recent: RecentExpired[]) {
  announcedRecentIds = new Set(recent.map((r) => r.id));
}

function renderRespawns(timers: RespawnTimer[], zone: string | null) {
  lastRespawns = timers;
  lastRespawnZone = zone;
  const roleLabel = document.getElementById("role-label");
  if (roleLabel && overlayRole === "respawns") {
    roleLabel.textContent = "Respawns";
    roleLabel.hidden = false;
  }
  syncOverlayZoneSelect(zone);
  const box = document.getElementById("timers")!;
  if (!timers.length) {
    const empty = zone
      ? `No respawn timers in ${escapeHtml(zone)}…`
      : "Select a zone (settings or dropdown) to track respawns…";
    box.innerHTML = `<div class="empty-state">${empty}</div>`;
    return;
  }
  box.innerHTML = `<div class="ogroup">${timers.map(renderRespawnRow).join("")}</div>`;
}

function render(timers: ActiveTimer[], recent: RecentExpired[] = []) {
  if (overlayRole === "respawns") return;
  lastTimers = timers;
  lastRecent = recent;
  announceNewWornOff(recent);
  const visibleTimers = filterForRole(timers);
  const visibleRecent = filterForRole(recent);
  const box = document.getElementById("timers")!;
  const showRecent = showRecentlyWoreOff && visibleRecent.length > 0;
  if (!visibleTimers.length && !showRecent) {
    const emptyMsg =
      overlayRole === "enemies" ? "No enemy timers…" : "Waiting for spells…";
    box.innerHTML = `<div class="empty-state">${emptyMsg}</div>`;
    return;
  }
  const parts: string[] = [];
  if (visibleTimers.length) {
    parts.push(renderGroupedActive(visibleTimers));
  }
  if (showRecent) {
    parts.push(`<div class="orecent-section">
      <div class="orecent-label">Recently wore off</div>
      ${renderGroupedRecent(visibleRecent)}
    </div>`);
  }
  box.innerHTML = parts.join("");
}
function syncLockedChrome() {
  document.body.classList.toggle("is-locked", locked);
}
async function loadSpellIcons() {
  try {
    const list = await invoke<SpellDef[]>("get_spells");
    iconBySpell = new Map(
      list.filter((s) => s.spellicon).map((s) => [s.name, s.spellicon!])
    );
  } catch {
    iconBySpell = new Map();
  }
}
window.addEventListener("DOMContentLoaded", async () => {
  const win = getCurrentWindow();
  if (win.label === "overlay-enemies") overlayRole = "enemies";
  else if (win.label === "overlay-respawns") overlayRole = "respawns";
  else overlayRole = "main";

  if (overlayRole !== "respawns") {
    await loadSpellIcons();
  }

  const config = await invoke<AppConfig>("get_config");
  locked = config.overlay_locked;
  applyConfig(config);
  syncLockedChrome();

  document.querySelector(".overlay-titlebar")?.addEventListener("mousedown", async (e) => {
    if (locked) return;
    if ((e.target as HTMLElement | null)?.closest?.("button, a, input, select")) return;
    try {
      await win.startDragging();
    } catch {
      /* ignore */
    }
  });

  document.getElementById("timers")!.addEventListener("contextmenu", async (e) => {
    if (locked || !rightClickDismiss) return;
    const row = (e.target as HTMLElement | null)?.closest?.(".otimer:not(.otimer-recent)") as
      | HTMLElement
      | null;
    const id = row?.dataset.timerId;
    if (!id) return;
    e.preventDefault();
    if (overlayRole === "respawns") {
      await invoke("dismiss_respawn", { id });
    } else {
      await invoke("dismiss_timer", { id });
    }
  });

  await listen<boolean>("overlay-lock-changed", (event) => {
    locked = event.payload;
    syncLockedChrome();
  });

  if (overlayRole === "respawns") {
    try {
      campsCache = await invoke<CampsFile>("get_camps");
    } catch {
      campsCache = null;
    }
    const zoneSelect = document.getElementById("zone-select") as HTMLSelectElement;
    zoneSelect.addEventListener("change", async () => {
      await invoke("set_respawn_zone", { zone: zoneSelect.value });
    });
    zoneSelect.addEventListener("mousedown", (e) => e.stopPropagation());

    const payload = await invoke<RespawnsPayload>("get_respawns");
    renderRespawns(payload.timers, payload.zone);
    await listen<RespawnsPayload>("respawns-updated", (event) => {
      renderRespawns(event.payload.timers, event.payload.zone);
    });
    await listen<AppConfig>("config-updated", (event) => {
      applyConfig(event.payload);
      if (event.payload.respawn_zone !== undefined) {
        syncOverlayZoneSelect(event.payload.respawn_zone || lastRespawnZone);
      }
      renderRespawns(lastRespawns, lastRespawnZone);
    });
  } else {
    const payload = await invoke<TimersPayload>("get_timers");
    seedAnnouncedRecent(payload.recent_expired ?? []);
    if (overlayRole === "main") primeSpeech();
    render(payload.timers, payload.recent_expired ?? []);
    await listen<TimersPayload>("timers-updated", (event) => {
      render(event.payload.timers, event.payload.recent_expired ?? []);
    });
    if (overlayRole === "main") {
      await listen<{ id: string; spell: string }>("timer-dismissed", (event) => {
        const spell = (event.payload.spell || "").trim();
        if (!spell) return;
        speakAnnouncement(`${spell} dismissed`);
      });
    }
    await listen<AppConfig>("config-updated", (event) => {
      applyConfig(event.payload);
      render(lastTimers, lastRecent);
    });
  }
});

