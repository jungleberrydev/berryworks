mod combat_parse;
mod engine;
mod log_suggest;
mod loot;
mod meter;
mod parser;
mod pets;
mod respawn;
mod spawn_db;
mod spell_db;
mod tailer;

use engine::{ActiveTimer, RecentExpired, TimerEngine};
use loot::{LootEngine, LootSnapshot, LootSyncResult};
use meter::{MeterEngine, MeterSnapshot};
use parser::parse_line;
use respawn::{RespawnEngine, RespawnTimer};
use spawn_db::{load_camps, CampsFile};
use spell_db::{
    load_config, load_spells, normalize_config, save_config, seed_watched_rares, AppConfig,
    SpellDef,
};
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tailer::{start_tailer, TailCommand};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

const OVERLAY_MAIN: &str = "overlay";
const OVERLAY_ENEMIES: &str = "overlay-enemies";
const OVERLAY_RESPAWNS: &str = "overlay-respawns";
const OVERLAY_ALERTS: &str = "overlay-alerts";
const OVERLAY_METER: &str = "overlay-meter";
const OVERLAY_WINDOWS: [&str; 5] = [
    OVERLAY_MAIN,
    OVERLAY_ENEMIES,
    OVERLAY_RESPAWNS,
    OVERLAY_ALERTS,
    OVERLAY_METER,
];
const WINDOW_STATE_FILE: &str = ".window-state.json";

/// Persist position/size (and maximized for settings). Skip visible/decorations so
/// overlay lock + separate-enemy / respawn show/hide stay authoritative.
const WINDOW_STATE_FLAGS: StateFlags =
    StateFlags::SIZE.union(StateFlags::POSITION).union(StateFlags::MAXIMIZED);

/// Last-known overlay geometry. The window-state plugin restores in `on_window_ready`,
/// then overlay lock/shadow/show races with Windows `CW_USEDEFAULT` placement and can
/// both move the window and poison the plugin cache via `Moved` events.
#[derive(Clone, serde::Deserialize)]
struct SavedWindowGeom {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

struct OverlayGeomStore {
    geoms: Mutex<HashMap<String, SavedWindowGeom>>,
    /// Ignore `Moved` until startup restore has been applied (creation cascade).
    restore_done: Mutex<bool>,
}

struct AppState {
    spells: Vec<SpellDef>,
    camps: CampsFile,
    config: Mutex<AppConfig>,
    engine: Mutex<TimerEngine>,
    respawn: Mutex<RespawnEngine>,
    loot: Mutex<LootEngine>,
    meter: Mutex<MeterEngine>,
    tail_cmd: Mutex<Option<mpsc::Sender<TailCommand>>>,
}

#[derive(Clone, serde::Serialize)]
struct TimersPayload {
    timers: Vec<ActiveTimer>,
    recent_expired: Vec<RecentExpired>,
}

#[derive(Clone, serde::Serialize)]
struct RespawnsPayload {
    timers: Vec<RespawnTimer>,
    zone: Option<String>,
}

fn snapshot_timers(engine: &mut TimerEngine, recent_ttl_secs: u64) -> TimersPayload {
    engine.clear_expired(recent_ttl_secs);
    TimersPayload {
        timers: engine.timers().to_vec(),
        recent_expired: engine.recent_expired().to_vec(),
    }
}

fn snapshot_respawns(engine: &mut RespawnEngine) -> RespawnsPayload {
    engine.clear_expired();
    RespawnsPayload {
        timers: engine.visible_timers(),
        zone: engine.current_zone().map(|s| s.to_string()),
    }
}

fn emit_timers(app: &AppHandle, state: &AppState) {
    let recent_ttl_secs = state
        .config
        .lock()
        .unwrap()
        .overlay
        .recently_wore_off_secs_clamped();
    let payload = {
        let mut engine = state.engine.lock().unwrap();
        snapshot_timers(&mut engine, recent_ttl_secs)
    };
    let _ = app.emit("timers-updated", payload);
}

fn emit_respawns(app: &AppHandle, state: &AppState) {
    let payload = {
        let mut engine = state.respawn.lock().unwrap();
        snapshot_respawns(&mut engine)
    };
    let _ = app.emit("respawns-updated", payload);
}

#[derive(Clone, serde::Serialize)]
struct OverlayAlert {
    id: String,
    title: String,
    detail: String,
    kind: String,
}

fn emit_overlay_alert(app: &AppHandle, title: &str, detail: &str, kind: &str) {
    let _ = app.emit(
        "overlay-alert",
        OverlayAlert {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            detail: detail.to_string(),
            kind: kind.to_string(),
        },
    );
}

fn emit_loot(app: &AppHandle, state: &AppState) {
    let payload = {
        let engine = state.loot.lock().unwrap();
        engine.snapshot("")
    };
    let _ = app.emit("loot-updated", payload);
}

fn emit_meter(app: &AppHandle, state: &AppState) {
    let payload = {
        let mut meter = state.meter.lock().unwrap();
        meter.mark_emitted();
        meter.snapshot()
    };
    let _ = app.emit("meter-updated", payload);
}

fn apply_meter_identity(state: &AppState, config: &AppConfig) {
    let name = log_suggest::character_name_from_log_path(&config.log_path);
    state
        .meter
        .lock()
        .unwrap()
        .set_identity(&name, &config.my_pet_name);
}

fn apply_line(app: &AppHandle, state: &AppState, line: &str) {
    let event = parse_line(line);

    // Keep character_level in sync with EQ level-ups so level-scaled buffs
    // (Chloroplast, Celerity, …) don't drift ~18s per missed level.
    if let parser::LogEvent::LevelUp { level } = &event {
        let mut config = state.config.lock().unwrap();
        if *level > 0 && config.character_level != *level {
            config.character_level = *level;
            let _ = save_config(&config);
            let snapshot = config.clone();
            drop(config);
            let _ = app.emit("config-updated", &snapshot);
        }
    }

    let config = state.config.lock().unwrap().clone();
    let (spell_changed, charm_alert, invis_alert) = {
        let mut engine = state.engine.lock().unwrap();
        let changed = engine.handle(event.clone(), &state.spells, &config);
        let charm = engine.take_charm_break_alert();
        let invis = engine.take_invis_break_alert();
        (changed, charm, invis)
    };
    let respawn_changed = {
        let mut engine = state.respawn.lock().unwrap();
        engine.handle(event.clone(), &state.camps, &config)
    };
    let loot_changed = {
        let mut engine = state.loot.lock().unwrap();
        // Keep loot zone aligned with respawn / settings zone when possible.
        if engine.current_zone().is_none() && !config.respawn_zone.is_empty() {
            engine.set_zone(&config.respawn_zone);
        }
        let changed = engine.handle(event.clone(), &config);
        if changed {
            let _ = engine.flush_if_dirty();
        }
        changed
    };
    let charmed = {
        let engine = state.engine.lock().unwrap();
        engine.charmed_targets()
    };
    apply_meter_identity(state, &config);
    let meter_changed = {
        let mut meter = state.meter.lock().unwrap();
        if meter.current_zone().is_none() && !config.respawn_zone.is_empty() {
            meter.set_zone(&config.respawn_zone);
        }
        meter.handle(&event, &charmed)
    };
    if spell_changed {
        emit_timers(app, state);
    }
    if let Some(alert) = charm_alert {
        let _ = app.emit("charm-broke", &alert);
        if config.overlay.charm_break_alerts {
            emit_overlay_alert(app, "Charm break!", &alert.target, "charm");
        }
    }
    if let Some(alert) = invis_alert {
        let _ = app.emit("invis-broke", &alert);
        if config.overlay.invis_break_alerts {
            let (title, kind) = if alert.fading {
                ("Invis fading!", "invis-fading")
            } else {
                match alert.kind.as_str() {
                    "ivu" => ("IVU wore off!", "ivu"),
                    "iva" => ("IVA wore off!", "iva"),
                    _ => ("Invis wore off!", "invis"),
                }
            };
            emit_overlay_alert(app, title, "", kind);
        }
    }
    if respawn_changed {
        // Persist zone from log so settings / restart stay in sync.
        if matches!(&event, parser::LogEvent::ZoneChange { .. }) {
            let zone_name = state
                .respawn
                .lock()
                .unwrap()
                .current_zone()
                .unwrap_or("")
                .to_string();
            {
                let mut loot = state.loot.lock().unwrap();
                loot.set_zone(&zone_name);
            }
            {
                let mut meter = state.meter.lock().unwrap();
                meter.set_zone(&zone_name);
            }
            let mut cfg = state.config.lock().unwrap();
            if cfg.respawn_zone != zone_name {
                cfg.respawn_zone = zone_name;
                let _ = save_config(&cfg);
                let snapshot = cfg.clone();
                drop(cfg);
                let _ = app.emit("config-updated", &snapshot);
            }
        }
        emit_respawns(app, state);
    }
    if loot_changed {
        emit_loot(app, state);
    }
    if meter_changed {
        let should = state.meter.lock().unwrap().should_emit();
        if should {
            emit_meter(app, state);
        }
    }
}

fn is_overlay_label(label: &str) -> bool {
    OVERLAY_WINDOWS.contains(&label)
}

fn geom_is_usable(g: &SavedWindowGeom) -> bool {
    g.width > 0 && g.height > 0 && g.x > -10_000 && g.y > -10_000
}

fn window_state_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(WINDOW_STATE_FILE))
}

fn load_saved_window_geoms(app: &AppHandle) -> HashMap<String, SavedWindowGeom> {
    let Some(path) = window_state_path(app) else {
        return HashMap::new();
    };
    let Ok(data) = std::fs::read(path) else {
        return HashMap::new();
    };
    let parsed: HashMap<String, SavedWindowGeom> = serde_json::from_slice(&data).unwrap_or_default();
    parsed
        .into_iter()
        .filter(|(label, g)| is_overlay_label(label) && geom_is_usable(g))
        .collect()
}

fn monitor_intersects(
    monitor: &tauri::Monitor,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> bool {
    let PhysicalPosition { x, y } = *monitor.position();
    let PhysicalSize { width, height } = *monitor.size();
    let left = x;
    let right = x + width as i32;
    let top = y;
    let bottom = y + height as i32;
    [
        (position.x, position.y),
        (position.x + size.width as i32, position.y),
        (position.x, position.y + size.height as i32),
        (
            position.x + size.width as i32,
            position.y + size.height as i32,
        ),
    ]
    .into_iter()
    .any(|(px, py)| px >= left && px < right && py >= top && py < bottom)
}

fn apply_saved_geom(win: &WebviewWindow, g: &SavedWindowGeom) {
    if !geom_is_usable(g) {
        return;
    }
    let mut x = g.x;
    let mut y = g.y;
    let position = PhysicalPosition { x, y };
    let size = PhysicalSize {
        width: g.width,
        height: g.height,
    };
    let on_screen = win
        .available_monitors()
        .ok()
        .map(|monitors| {
            monitors
                .iter()
                .any(|m| monitor_intersects(m, position, size))
        })
        .unwrap_or(true);
    if !on_screen {
        if let Ok(Some(primary)) = win.primary_monitor() {
            let origin = primary.position();
            x = origin.x + 40;
            y = origin.y + 40;
        }
    }
    let _ = win.set_position(PhysicalPosition { x, y });
    let _ = win.set_size(PhysicalSize {
        width: g.width,
        height: g.height,
    });
}

fn apply_saved_overlay_geoms(app: &AppHandle, geoms: &HashMap<String, SavedWindowGeom>) {
    for (label, g) in geoms {
        if let Some(win) = app.get_webview_window(label) {
            apply_saved_geom(&win, g);
        }
    }
}

fn remember_overlay_geom(app: &AppHandle, label: &str, g: SavedWindowGeom) {
    if !geom_is_usable(&g) {
        return;
    }
    if let Some(store) = app.try_state::<OverlayGeomStore>() {
        store.geoms.lock().unwrap().insert(label.to_string(), g);
    }
}

fn overlay_geom_for(app: &AppHandle, label: &str) -> Option<SavedWindowGeom> {
    app.try_state::<OverlayGeomStore>()
        .and_then(|store| store.geoms.lock().unwrap().get(label).cloned())
}

fn overlay_restore_done(app: &AppHandle) -> bool {
    app.try_state::<OverlayGeomStore>()
        .map(|store| *store.restore_done.lock().unwrap())
        .unwrap_or(false)
}

/// Re-apply last-known overlay geometry after show/chrome so `SetWindowPos` sticks.
fn restore_overlay_geom(app: &AppHandle, label: &str) {
    if let (Some(win), Some(g)) = (app.get_webview_window(label), overlay_geom_for(app, label)) {
        apply_saved_geom(&win, &g);
    } else if let Some(win) = app.get_webview_window(label) {
        let _ = win.restore_state(WINDOW_STATE_FLAGS);
    }
}

fn snapshot_overlay_pins(app: &AppHandle) -> Vec<(WebviewWindow, SavedWindowGeom)> {
    let mut pins = Vec::new();
    for label in OVERLAY_WINDOWS {
        let Some(win) = app.get_webview_window(label) else {
            continue;
        };
        let Ok(pos) = win.outer_position() else {
            continue;
        };
        let Ok(size) = win.inner_size() else {
            continue;
        };
        let g = SavedWindowGeom {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        };
        if geom_is_usable(&g) {
            pins.push((win, g));
        }
    }
    pins
}

/// Lock/shadow/decorations change DWM styles and can move the window; pin geometry.
fn pin_overlay_positions(app: &AppHandle, apply: impl FnOnce()) {
    let pins = snapshot_overlay_pins(app);
    apply();
    for (win, g) in &pins {
        apply_saved_geom(win, g);
        if overlay_restore_done(app) {
            remember_overlay_geom(app, win.label(), g.clone());
        }
    }
}

/// Merge overlay geometry into `.window-state.json` after the plugin's Exit save,
/// so hidden windows (and destroy/`Moved` poison) cannot overwrite last placement.
fn persist_overlay_geoms(app: &AppHandle) {
    let Some(store) = app.try_state::<OverlayGeomStore>() else {
        return;
    };
    let geoms = store.geoms.lock().unwrap().clone();
    if geoms.is_empty() {
        return;
    }
    let Some(path) = window_state_path(app) else {
        return;
    };
    let mut root: serde_json::Value = std::fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let Some(obj) = root.as_object_mut() else {
        return;
    };
    for label in OVERLAY_WINDOWS {
        let Some(g) = geoms.get(label) else {
            continue;
        };
        if !geom_is_usable(g) {
            continue;
        }
        let entry = obj.entry(label.to_string()).or_insert_with(|| {
            serde_json::json!({
                "width": g.width,
                "height": g.height,
                "x": g.x,
                "y": g.y,
                "prev_x": g.x,
                "prev_y": g.y,
                "maximized": false,
                "visible": true,
                "decorated": false,
                "fullscreen": false
            })
        });
        if let Some(map) = entry.as_object_mut() {
            map.insert("x".into(), serde_json::json!(g.x));
            map.insert("y".into(), serde_json::json!(g.y));
            map.insert("width".into(), serde_json::json!(g.width));
            map.insert("height".into(), serde_json::json!(g.height));
        }
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&root) {
        let _ = std::fs::write(path, bytes);
    }
}

fn track_overlay_geometry(app: &AppHandle) {
    for label in OVERLAY_WINDOWS {
        let Some(win) = app.get_webview_window(label) else {
            continue;
        };
        let handle = app.clone();
        let label = label.to_string();
        let tracked = win.clone();
        win.on_window_event(move |event| {
            if !overlay_restore_done(&handle) {
                return;
            }
            match event {
                WindowEvent::Moved(pos) => {
                    if pos.x <= -10_000 || pos.y <= -10_000 {
                        return;
                    }
                    let size = tracked.inner_size().unwrap_or(PhysicalSize {
                        width: 0,
                        height: 0,
                    });
                    remember_overlay_geom(
                        &handle,
                        &label,
                        SavedWindowGeom {
                            x: pos.x,
                            y: pos.y,
                            width: size.width,
                            height: size.height,
                        },
                    );
                }
                WindowEvent::Resized(size) => {
                    if size.width == 0 || size.height == 0 {
                        return;
                    }
                    let pos = tracked.outer_position().unwrap_or(PhysicalPosition { x: 0, y: 0 });
                    remember_overlay_geom(
                        &handle,
                        &label,
                        SavedWindowGeom {
                            x: pos.x,
                            y: pos.y,
                            width: size.width,
                            height: size.height,
                        },
                    );
                }
                _ => {}
            }
        });
    }
}

/// Apply click-through + native DWM chrome for overlay windows.
///
/// Unlocked: receive mouse events and enable `shadow` so Windows hit-testing
/// / `startDragging` works on undecorated transparent windows.
/// Locked: ignore cursor events (click-through) and hide the DWM border/shadow
/// for a clean in-game look.
fn apply_overlay_lock(app: &AppHandle, locked: bool) {
    pin_overlay_positions(app, || {
        for label in OVERLAY_WINDOWS {
            if let Some(win) = app.get_webview_window(label) {
                let _ = win.set_ignore_cursor_events(locked);
                let _ = win.set_shadow(!locked);
                let _ = win.set_decorations(false);
            }
        }
    });
}

/// Show or hide the enemy overlay based on `overlay.separate_enemy_window`.
fn sync_enemy_overlay(app: &AppHandle, separate: bool, locked: bool) {
    let Some(win) = app.get_webview_window(OVERLAY_ENEMIES) else {
        return;
    };
    if separate {
        let _ = win.show();
        let _ = win.set_always_on_top(true);
        let _ = win.set_ignore_cursor_events(locked);
        let _ = win.set_shadow(!locked);
        let _ = win.set_decorations(false);
        restore_overlay_geom(app, OVERLAY_ENEMIES);
    } else {
        let _ = win.hide();
    }
}

/// Show or hide the respawn overlay based on `overlay.show_respawn_window`.
fn sync_respawn_overlay(app: &AppHandle, show: bool, locked: bool) {
    let Some(win) = app.get_webview_window(OVERLAY_RESPAWNS) else {
        return;
    };
    if show {
        let _ = win.show();
        let _ = win.set_always_on_top(true);
        let _ = win.set_ignore_cursor_events(locked);
        let _ = win.set_shadow(!locked);
        let _ = win.set_decorations(false);
        restore_overlay_geom(app, OVERLAY_RESPAWNS);
    } else {
        let _ = win.hide();
    }
}

/// Show or hide the alert overlay based on `overlay.show_alert_window`.
fn sync_alert_overlay(app: &AppHandle, show: bool, locked: bool) {
    let Some(win) = app.get_webview_window(OVERLAY_ALERTS) else {
        return;
    };
    if show {
        let _ = win.show();
        let _ = win.set_always_on_top(true);
        let _ = win.set_ignore_cursor_events(locked);
        let _ = win.set_shadow(!locked);
        let _ = win.set_decorations(false);
        restore_overlay_geom(app, OVERLAY_ALERTS);
    } else {
        let _ = win.hide();
    }
}

/// Show or hide the DPS meter overlay based on `overlay.show_meter_window`.
fn sync_meter_overlay(app: &AppHandle, show: bool, locked: bool) {
    let Some(win) = app.get_webview_window(OVERLAY_METER) else {
        return;
    };
    if show {
        let _ = win.show();
        let _ = win.set_always_on_top(true);
        let _ = win.set_ignore_cursor_events(locked);
        let _ = win.set_shadow(!locked);
        let _ = win.set_decorations(false);
        restore_overlay_geom(app, OVERLAY_METER);
    } else {
        let _ = win.hide();
    }
}

#[tauri::command]
fn get_spells(state: State<'_, AppState>) -> Vec<SpellDef> {
    state.spells.clone()
}

#[tauri::command]
fn get_camps(state: State<'_, AppState>) -> CampsFile {
    state.camps.clone()
}

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut config: AppConfig,
) -> Result<AppConfig, String> {
    normalize_config(&mut config);
    save_config(&config)?;
    let separate = config.overlay.separate_enemy_window;
    let show_respawn = config.overlay.show_respawn_window;
    let show_alerts = config.overlay.show_alert_window;
    let show_meter = config.overlay.show_meter_window;
    let locked = config.overlay_locked;
    let respawn_zone = config.respawn_zone.clone();
    {
        let mut locked_cfg = state.config.lock().unwrap();
        *locked_cfg = config.clone();
    }
    {
        let mut engine = state.respawn.lock().unwrap();
        engine.set_zone(&respawn_zone, &state.camps);
    }
    {
        let mut loot = state.loot.lock().unwrap();
        loot.set_zone(&respawn_zone);
    }
    apply_meter_identity(&state, &config);
    {
        let mut meter = state.meter.lock().unwrap();
        meter.set_zone(&respawn_zone);
    }

    if let Some(tx) = state.tail_cmd.lock().unwrap().as_ref() {
        if !config.log_path.is_empty() {
            let _ = tx.send(TailCommand::SetPath(config.log_path.clone().into()));
        }
    }
    sync_enemy_overlay(&app, separate, locked);
    sync_respawn_overlay(&app, show_respawn, locked);
    sync_alert_overlay(&app, show_alerts, locked);
    sync_meter_overlay(&app, show_meter, locked);
    apply_overlay_lock(&app, locked);
    let _ = app.emit("config-updated", &config);
    emit_respawns(&app, &state);
    emit_meter(&app, &state);
    Ok(config)
}

#[tauri::command]
fn get_timers(state: State<'_, AppState>) -> TimersPayload {
    let recent_ttl_secs = state
        .config
        .lock()
        .unwrap()
        .overlay
        .recently_wore_off_secs_clamped();
    let mut engine = state.engine.lock().unwrap();
    snapshot_timers(&mut engine, recent_ttl_secs)
}

#[tauri::command]
fn get_respawns(state: State<'_, AppState>) -> RespawnsPayload {
    let mut engine = state.respawn.lock().unwrap();
    snapshot_respawns(&mut engine)
}

#[tauri::command]
fn set_respawn_zone(
    app: AppHandle,
    state: State<'_, AppState>,
    zone: String,
) -> Result<RespawnsPayload, String> {
    {
        let mut engine = state.respawn.lock().unwrap();
        engine.set_zone(&zone, &state.camps);
    }
            {
                let mut loot = state.loot.lock().unwrap();
                loot.set_zone(&zone);
            }
            {
                let mut meter = state.meter.lock().unwrap();
                meter.set_zone(&zone);
            }
    let snapshot = {
        let mut config = state.config.lock().unwrap();
        config.respawn_zone = zone.trim().to_string();
        save_config(&config)?;
        config.clone()
    };
    let _ = app.emit("config-updated", &snapshot);
    let payload = {
        let mut engine = state.respawn.lock().unwrap();
        snapshot_respawns(&mut engine)
    };
    let _ = app.emit("respawns-updated", &payload);
    Ok(payload)
}

#[tauri::command]
fn clear_timers(app: AppHandle, state: State<'_, AppState>) {
    state.engine.lock().unwrap().clear_all();
    emit_timers(&app, &state);
}

#[tauri::command]
fn clear_respawns(app: AppHandle, state: State<'_, AppState>) {
    state.respawn.lock().unwrap().clear_all();
    emit_respawns(&app, &state);
}

#[tauri::command]
fn get_loot(state: State<'_, AppState>, query: Option<String>) -> LootSnapshot {
    let engine = state.loot.lock().unwrap();
    engine.snapshot(query.as_deref().unwrap_or(""))
}

#[tauri::command]
fn clear_loot(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut engine = state.loot.lock().unwrap();
        engine.clear_all()?;
    }
    emit_loot(&app, &state);
    Ok(())
}

#[tauri::command]
fn get_meter(state: State<'_, AppState>) -> MeterSnapshot {
    let config = state.config.lock().unwrap().clone();
    apply_meter_identity(&state, &config);
    state.meter.lock().unwrap().snapshot()
}

#[tauri::command]
fn reset_meter_session(app: AppHandle, state: State<'_, AppState>) -> MeterSnapshot {
    let config = state.config.lock().unwrap().clone();
    apply_meter_identity(&state, &config);
    {
        let mut meter = state.meter.lock().unwrap();
        meter.reset_session();
        if !config.respawn_zone.is_empty() {
            meter.set_zone(&config.respawn_zone);
        }
    }
    emit_meter(&app, &state);
    state.meter.lock().unwrap().snapshot()
}

#[tauri::command]
fn upload_loot(state: State<'_, AppState>) -> Result<LootSyncResult, String> {
    let config = {
        let mut cfg = state.config.lock().unwrap();
        normalize_config(&mut cfg);
        save_config(&cfg)?;
        cfg.clone()
    };
    if !config.loot_sync_enabled {
        return Ok(LootSyncResult {
            ok: false,
            message: "Enable community sync in Loot settings first.".into(),
            kills_added: None,
            drops_added: None,
        });
    }
    let upload_token = config.loot_upload_token.trim().to_string();
    let ops_key = config.loot_sync_key.trim().to_string();
    if upload_token.is_empty() && ops_key.is_empty() {
        return Ok(LootSyncResult {
            ok: false,
            message: "Sign in with Discord to upload (Loot tab).".into(),
            kills_added: None,
            drops_added: None,
        });
    }

    let payload = {
        let engine = state.loot.lock().unwrap();
        engine.export_for_sync(&config.loot_contributor_id)
    };
    if payload.mobs.is_empty() {
        return Ok(LootSyncResult {
            ok: false,
            message: "No local loot to upload yet.".into(),
            kills_added: None,
            drops_added: None,
        });
    }

    let url = format!(
        "{}/api/loot/ingest",
        config.loot_sync_url.trim_end_matches('/')
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let mut request = client.post(&url).json(&payload);
    if !upload_token.is_empty() {
        request = request.header("Authorization", format!("Bearer {upload_token}"));
    } else {
        request = request
            .header("Authorization", format!("Bearer {ops_key}"))
            .header("X-Berryworks-Key", ops_key);
    }
    let response = request
        .send()
        .map_err(|e| format!("Upload failed: {e}"))?;

    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("error")?.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| text.chars().take(200).collect());
        return Ok(LootSyncResult {
            ok: false,
            message: format!("Server {status}: {msg}"),
            kills_added: None,
            drops_added: None,
        });
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "ok": true }));
    Ok(LootSyncResult {
        ok: true,
        message: "Uploaded to Norrath Roster.".into(),
        kills_added: parsed
            .get("killsAdded")
            .and_then(|v| v.as_u64())
            .or_else(|| parsed.get("kills_added").and_then(|v| v.as_u64())),
        drops_added: parsed
            .get("dropsAdded")
            .and_then(|v| v.as_u64())
            .or_else(|| parsed.get("drops_added").and_then(|v| v.as_u64())),
    })
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct LootDiscordUser {
    id: String,
    username: String,
    #[serde(rename = "globalName")]
    global_name: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct LootDiscordLoginResult {
    user: LootDiscordUser,
}

/// Open Discord OAuth in the browser and poll until Berryworks issues an upload token.
#[tauri::command]
fn login_loot_discord(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LootDiscordLoginResult, String> {
    use tauri_plugin_opener::OpenerExt;

    let base = {
        let mut cfg = state.config.lock().unwrap();
        normalize_config(&mut cfg);
        cfg.loot_sync_url.trim_end_matches('/').to_string()
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let start_url = format!("{base}/api/loot/auth/start");
    let start_res = client
        .post(&start_url)
        .send()
        .map_err(|e| format!("Could not start Discord login: {e}"))?;
    let start_status = start_res.status();
    let start_text = start_res.text().unwrap_or_default();
    if !start_status.is_success() {
        let msg = serde_json::from_str::<serde_json::Value>(&start_text)
            .ok()
            .and_then(|v| v.get("error")?.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| start_text.chars().take(200).collect());
        if start_status.as_u16() == 404 {
            return Err(format!(
                "Login start failed (404): Discord loot auth is not available at {base}. \
                 Deploy norrath-roster with loot auth enabled."
            ));
        }
        return Err(format!("Login start failed ({start_status}): {msg}"));
    }
    let start_json: serde_json::Value =
        serde_json::from_str(&start_text).map_err(|e| e.to_string())?;
    let session_id = start_json
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Login start missing sessionId".to_string())?
        .to_string();
    let authorize_url = start_json
        .get("authorizeUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Login start missing authorizeUrl".to_string())?
        .to_string();

    app.opener()
        .open_url(authorize_url, None::<&str>)
        .map_err(|e| format!("Could not open browser: {e}"))?;

    let poll_url = format!("{base}/api/loot/auth/poll");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5 * 60);
    loop {
        if std::time::Instant::now() > deadline {
            return Err("Discord login timed out. Try again.".into());
        }
        thread::sleep(Duration::from_millis(1500));
        let poll_res = client
            .get(&poll_url)
            .query(&[("sessionId", session_id.as_str())])
            .send()
            .map_err(|e| format!("Login poll failed: {e}"))?;
        if !poll_res.status().is_success() {
            continue;
        }
        let poll_json: serde_json::Value = poll_res.json().map_err(|e| e.to_string())?;
        let status = poll_json
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match status {
            "pending" => continue,
            "ready" => {
                let token = poll_json
                    .get("token")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Login ready but token missing".to_string())?
                    .to_string();
                let user_val = poll_json
                    .get("user")
                    .cloned()
                    .ok_or_else(|| "Login ready but user missing".to_string())?;
                let user: LootDiscordUser =
                    serde_json::from_value(user_val).map_err(|e| e.to_string())?;
                {
                    let mut cfg = state.config.lock().unwrap();
                    cfg.loot_upload_token = token;
                    cfg.loot_discord_user_id = user.id.clone();
                    cfg.loot_discord_username = user.username.clone();
                    cfg.loot_discord_global_name =
                        user.global_name.clone().unwrap_or_default();
                    cfg.loot_sync_enabled = true;
                    normalize_config(&mut cfg);
                    save_config(&cfg)?;
                }
                return Ok(LootDiscordLoginResult { user });
            }
            "banned" => return Err("This Discord account is banned.".into()),
            "expired" | "failed" | "consumed" => {
                return Err("Discord login expired or failed. Try again.".into());
            }
            other => {
                return Err(format!("Unexpected login status: {other}"));
            }
        }
    }
}

#[tauri::command]
fn logout_loot_discord(state: State<'_, AppState>) -> Result<(), String> {
    let (base, token) = {
        let cfg = state.config.lock().unwrap();
        (
            cfg.loot_sync_url.trim_end_matches('/').to_string(),
            cfg.loot_upload_token.trim().to_string(),
        )
    };
    if !token.is_empty() {
        if let Ok(client) = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
        {
            let _ = client
                .post(format!("{base}/api/loot/auth/revoke"))
                .header("Authorization", format!("Bearer {token}"))
                .send();
        }
    }
    let mut cfg = state.config.lock().unwrap();
    cfg.loot_upload_token.clear();
    cfg.loot_discord_username.clear();
    cfg.loot_discord_global_name.clear();
    cfg.loot_discord_user_id.clear();
    normalize_config(&mut cfg);
    save_config(&cfg)?;
    Ok(())
}

#[tauri::command]
fn dismiss_timer(app: AppHandle, state: State<'_, AppState>, id: String) -> bool {
    let removed = state.engine.lock().unwrap().dismiss_timer(&id);
    if let Some(timer) = removed {
            let _ = app.emit(
            "timer-dismissed",
            serde_json::json!({ "id": timer.id, "spell": timer.spell }),
        );
        emit_timers(&app, &state);
        true
    } else {
        false
    }
}

#[tauri::command]
fn dismiss_respawn(app: AppHandle, state: State<'_, AppState>, id: String) -> bool {
    let removed = state.respawn.lock().unwrap().dismiss(&id);
    if removed {
        emit_respawns(&app, &state);
    }
    removed
}

#[tauri::command]
fn set_overlay_locked(app: AppHandle, state: State<'_, AppState>, locked: bool) -> Result<(), String> {
    let config_snapshot = {
        let mut config = state.config.lock().unwrap();
        config.overlay_locked = locked;
        save_config(&config)?;
        config.clone()
    };
    apply_overlay_lock(&app, locked);
    let _ = app.emit("overlay-lock-changed", locked);
    let _ = app.emit("config-updated", &config_snapshot);
    Ok(())
}

#[tauri::command]
fn preview_overlay_alert(app: AppHandle, state: State<'_, AppState>, kind: Option<String>) {
    let locked = state.config.lock().unwrap().overlay_locked;
    sync_alert_overlay(&app, true, locked);
    match kind.as_deref().unwrap_or("charm") {
        "invis" => emit_overlay_alert(&app, "Invis wore off!", "", "invis"),
        "invis-fading" => emit_overlay_alert(&app, "Invis fading!", "", "invis-fading"),
        "ivu" => emit_overlay_alert(&app, "IVU wore off!", "", "ivu"),
        "iva" => emit_overlay_alert(&app, "IVA wore off!", "", "iva"),
        _ => emit_overlay_alert(&app, "Charm break!", "a gnoll", "charm"),
    }
}
fn shutdown_with_overlay(app: &AppHandle) {
    // Flush plugin state, then overlay geoms (hidden windows / destroy races).
    // Do not destroy overlays first: WM_MOVE during teardown poisons saved coords.
    let _ = app.save_window_state(WINDOW_STATE_FLAGS);
    persist_overlay_geoms(app);
    app.exit(0);
}

#[tauri::command]
fn inject_log_line(app: AppHandle, state: State<'_, AppState>, line: String) {
    apply_line(&app, &state, &line);
}

#[tauri::command]
fn suggest_log_paths() -> Vec<log_suggest::SuggestedLog> {
    log_suggest::suggest_log_paths()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let spells = load_spells().expect("Failed to load spells.json");
    let camps = load_camps().expect("Failed to load camps.json");
    let mut config = load_config(&spells);
    seed_watched_rares(&mut config, &camps);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(WINDOW_STATE_FLAGS)
                // Restore overlays ourselves after lock/shadow/show; the plugin's
                // on_window_ready restore loses to Windows cascade + DWM style changes.
                .skip_initial_state(OVERLAY_MAIN)
                .skip_initial_state(OVERLAY_ENEMIES)
                .skip_initial_state(OVERLAY_RESPAWNS)
                .skip_initial_state(OVERLAY_ALERTS)
                .skip_initial_state(OVERLAY_METER)
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus existing instance instead of spawning a second overlay process.
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.set_focus();
                let _ = main.unminimize();
                let _ = main.show();
            }
        }))
        .manage(AppState {
            spells,
            camps,
            config: Mutex::new(config),
            engine: Mutex::new(TimerEngine::new()),
            respawn: Mutex::new(RespawnEngine::new()),
            loot: Mutex::new(LootEngine::new()),
            meter: Mutex::new(MeterEngine::new()),
            tail_cmd: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_spells,
            get_camps,
            get_config,
            save_settings,
            get_timers,
            get_respawns,
            set_respawn_zone,
            clear_timers,
            clear_respawns,
            get_loot,
            clear_loot,
            get_meter,
            reset_meter_session,
            upload_loot,
            login_loot_discord,
            logout_loot_discord,
            dismiss_timer,
            dismiss_respawn,
            set_overlay_locked,
            preview_overlay_alert,
            inject_log_line,
            suggest_log_paths
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            let saved_overlay_geoms = load_saved_window_geoms(app.handle());
            app.manage(OverlayGeomStore {
                geoms: Mutex::new(saved_overlay_geoms.clone()),
                restore_done: Mutex::new(saved_overlay_geoms.is_empty()),
            });
            track_overlay_geometry(app.handle());

            let (line_tx, line_rx) = mpsc::channel::<String>();
            let cmd_tx = start_tailer(line_tx);
            {
                let config = state.config.lock().unwrap();
                if !config.log_path.is_empty() {
                    let _ = cmd_tx.send(TailCommand::SetPath(config.log_path.clone().into()));
                }
                if !config.respawn_zone.is_empty() {
                    state
                        .respawn
                        .lock()
                        .unwrap()
                        .set_zone(&config.respawn_zone, &state.camps);
                    state.loot.lock().unwrap().set_zone(&config.respawn_zone);
                    state.meter.lock().unwrap().set_zone(&config.respawn_zone);
                }
                apply_meter_identity(&state, &config);
                apply_overlay_lock(app.handle(), config.overlay_locked);
                sync_enemy_overlay(
                    app.handle(),
                    config.overlay.separate_enemy_window,
                    config.overlay_locked,
                );
                sync_respawn_overlay(
                    app.handle(),
                    config.overlay.show_respawn_window,
                    config.overlay_locked,
                );
                sync_alert_overlay(
                    app.handle(),
                    config.overlay.show_alert_window,
                    config.overlay_locked,
                );
                sync_meter_overlay(
                    app.handle(),
                    config.overlay.show_meter_window,
                    config.overlay_locked,
                );
            }
            // After lock/show: apply last geometry (plugin restore already skipped).
            apply_saved_overlay_geoms(app.handle(), &saved_overlay_geoms);
            if !saved_overlay_geoms.is_empty() {
                let deferred_handle = app.handle().clone();
                let deferred_geoms = saved_overlay_geoms.clone();
                if app
                    .handle()
                    .run_on_main_thread(move || {
                        apply_saved_overlay_geoms(&deferred_handle, &deferred_geoms);
                        if let Some(store) = deferred_handle.try_state::<OverlayGeomStore>() {
                            *store.restore_done.lock().unwrap() = true;
                        }
                    })
                    .is_err()
                {
                    if let Some(store) = app.try_state::<OverlayGeomStore>() {
                        *store.restore_done.lock().unwrap() = true;
                    }
                }
            }
            *state.tail_cmd.lock().unwrap() = Some(cmd_tx);

            // Fan-in log lines onto the engine
            let handle2 = handle.clone();
            thread::spawn(move || {
                while let Ok(line) = line_rx.recv() {
                    let state = handle2.state::<AppState>();
                    apply_line(&handle2, &state, &line);
                }
            });

            // Periodic expiry sweep so overlay bars clear
            let handle3 = handle.clone();
            thread::spawn(move || loop {
                thread::sleep(Duration::from_millis(250));
                let state = handle3.state::<AppState>();
                {
                    let recent_ttl_secs = state
                        .config
                        .lock()
                        .unwrap()
                        .overlay
                        .recently_wore_off_secs_clamped();
                    let mut engine = state.engine.lock().unwrap();
                    engine.clear_expired(recent_ttl_secs);
                }
                emit_timers(&handle3, &state);
                {
                    let mut engine = state.respawn.lock().unwrap();
                    engine.clear_expired();
                }
                emit_respawns(&handle3, &state);
                let meter_tick = {
                    let mut meter = state.meter.lock().unwrap();
                    let closed = meter.tick();
                    closed || meter.has_active_fight()
                };
                if meter_tick {
                    emit_meter(&handle3, &state);
                }
            });

            // Keep only the canonical overlay windows; destroy any stray overlay*.
            let allowed = OVERLAY_WINDOWS;
            let labels: Vec<String> = app
                .webview_windows()
                .into_keys()
                .filter(|l| {
                    OVERLAY_WINDOWS.contains(&l.as_str()) || l.starts_with("overlay")
                })
                .collect();
            for label in labels {
                if allowed.contains(&label.as_str()) {
                    continue;
                }
                if let Some(win) = app.get_webview_window(&label) {
                    let _ = win.destroy();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label().to_string();
                // Closing settings (main) tears down overlays and exits.
                if label == "main" {
                    shutdown_with_overlay(window.app_handle());
                    return;
                }
                // Overlay close: optional windows hide (toggles reopen them).
                // The main timer overlay stays up.
                if is_overlay_label(&label) {
                    api.prevent_close();
                    if label == OVERLAY_MAIN {
                        let _ = window.show();
                    } else {
                        let _ = window.hide();
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                persist_overlay_geoms(app);
            }
        });
}
