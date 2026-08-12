mod engine;
mod log_suggest;
mod loot;
mod parser;
mod pets;
mod respawn;
mod spawn_db;
mod spell_db;
mod tailer;

use engine::{ActiveTimer, RecentExpired, TimerEngine};
use loot::{LootEngine, LootSnapshot, LootSyncResult};
use parser::parse_line;
use respawn::{RespawnEngine, RespawnTimer};
use spawn_db::{load_camps, CampsFile};
use spell_db::{
    load_config, load_spells, normalize_config, save_config, seed_watched_rares, AppConfig,
    SpellDef,
};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tailer::{start_tailer, TailCommand};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

const OVERLAY_MAIN: &str = "overlay";
const OVERLAY_ENEMIES: &str = "overlay-enemies";
const OVERLAY_RESPAWNS: &str = "overlay-respawns";

/// Persist position/size (and maximized for settings). Skip visible/decorations so
/// overlay lock + separate-enemy / respawn show/hide stay authoritative.
const WINDOW_STATE_FLAGS: StateFlags =
    StateFlags::SIZE.union(StateFlags::POSITION).union(StateFlags::MAXIMIZED);

struct AppState {
    spells: Vec<SpellDef>,
    camps: CampsFile,
    config: Mutex<AppConfig>,
    engine: Mutex<TimerEngine>,
    respawn: Mutex<RespawnEngine>,
    loot: Mutex<LootEngine>,
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

fn emit_loot(app: &AppHandle, state: &AppState) {
    let payload = {
        let engine = state.loot.lock().unwrap();
        engine.snapshot("")
    };
    let _ = app.emit("loot-updated", payload);
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
    let spell_changed = {
        let mut engine = state.engine.lock().unwrap();
        engine.handle(event.clone(), &state.spells, &config)
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
    if spell_changed {
        emit_timers(app, state);
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
}

fn overlay_labels(app: &AppHandle) -> Vec<String> {
    app.webview_windows()
        .into_keys()
        .filter(|label| {
            label == OVERLAY_MAIN
                || label == OVERLAY_ENEMIES
                || label == OVERLAY_RESPAWNS
                || label.starts_with("overlay")
        })
        .collect()
}

/// Apply click-through + native DWM chrome for overlay windows.
///
/// Unlocked: receive mouse events and enable `shadow` so Windows hit-testing
/// / `startDragging` works on undecorated transparent windows.
/// Locked: ignore cursor events (click-through) and hide the DWM border/shadow
/// for a clean in-game look.
fn apply_overlay_lock(app: &AppHandle, locked: bool) {
    for label in [OVERLAY_MAIN, OVERLAY_ENEMIES, OVERLAY_RESPAWNS] {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.set_ignore_cursor_events(locked);
            let _ = win.set_shadow(!locked);
            let _ = win.set_decorations(false);
        }
    }
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

    if let Some(tx) = state.tail_cmd.lock().unwrap().as_ref() {
        if !config.log_path.is_empty() {
            let _ = tx.send(TailCommand::SetPath(config.log_path.clone().into()));
        }
    }
    sync_enemy_overlay(&app, separate, locked);
    sync_respawn_overlay(&app, show_respawn, locked);
    apply_overlay_lock(&app, locked);
    let _ = app.emit("config-updated", &config);
    emit_respawns(&app, &state);
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
fn show_overlay(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let (separate, show_respawn, locked) = {
        let config = state.config.lock().unwrap();
        (
            config.overlay.separate_enemy_window,
            config.overlay.show_respawn_window,
            config.overlay_locked,
        )
    };
    if let Some(win) = app.get_webview_window(OVERLAY_MAIN) {
        win.show().map_err(|e| e.to_string())?;
        win.set_always_on_top(true).map_err(|e| e.to_string())?;
    }
    apply_overlay_lock(&app, locked);
    if separate {
        sync_enemy_overlay(&app, true, locked);
    }
    if show_respawn {
        sync_respawn_overlay(&app, true, locked);
    }
    Ok(())
}

/// Tear down overlay window(s) and exit the process (used when main closes).
fn shutdown_with_overlay(app: &AppHandle) {
    // Flush geometry before destroying overlays so the next launch restores it.
    let _ = app.save_window_state(WINDOW_STATE_FLAGS);
    for label in overlay_labels(app) {
        if let Some(win) = app.get_webview_window(&label) {
            let _ = win.destroy();
        }
    }
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
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(WINDOW_STATE_FLAGS)
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
            upload_loot,
            login_loot_discord,
            logout_loot_discord,
            dismiss_timer,
            dismiss_respawn,
            set_overlay_locked,
            show_overlay,
            inject_log_line,
            suggest_log_paths
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();

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
                }
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
            });

            // Keep only the canonical overlay windows; destroy any stray overlay*.
            let allowed = [OVERLAY_MAIN, OVERLAY_ENEMIES, OVERLAY_RESPAWNS];
            let labels: Vec<String> = app
                .webview_windows()
                .into_keys()
                .filter(|l| {
                    l == OVERLAY_MAIN
                        || l == OVERLAY_ENEMIES
                        || l == OVERLAY_RESPAWNS
                        || l.starts_with("overlay")
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
                // Overlay close: hide so Show Overlay / toggles can reopen it.
                if label == OVERLAY_MAIN
                    || label == OVERLAY_ENEMIES
                    || label == OVERLAY_RESPAWNS
                {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
