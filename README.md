# Berryworks

EverQuest Legends toolkit. Timers are the current main feature: tails your character log, starts timers when watched spells land (with target names), and shows remaining time on a freely movable always-on-top window.

## Download & install (Windows)

1. Open the latest [GitHub Release](../../releases/latest) and download the **NSIS installer** (`Berryworks_*_x64-setup.exe`).
2. Run the installer. Windows **SmartScreen** may warn on unsigned builds — choose **More info → Run anyway** if you trust the release.
3. Open **Berryworks**. On the General tab, pick a detected character log (or **Browse…**), set your level, then **Save Settings**.
4. In game, type `/log on` so EverQuest writes your character log.

Logs are usually named `eqlog_<Character>_<Server>.txt` under the game’s `Logs` folder (Berryworks also scans common EverQuest / Daybreak install paths).

Settings live in `%APPDATA%\berry-timers\`.

## Requirements (end users)

- Windows 10/11
- WebView2 (usually already on Windows 11; the installer can bootstrap it if needed)

## In-game

1. Type `/log on` so EverQuest writes a character log.
2. In **Berryworks**, select that file and **Save Settings**.
3. Set your **character level**. Toggle watched spells under each class (tier comes from cast-line Roman numerals, e.g. `Spirit of Wolf V`).
4. Drag the overlay by its title bar. Click **Lock** so mouse clicks pass through to the game; **Unlock** to move or interact again. With **Right-click overlay timer to dismiss** enabled (`overlay.right_click_dismiss`, default on), right-click a timer row while unlocked to remove it. With **Show recently wore off** enabled (`overlay.show_recently_wore_off`, default on), expired or cleared timers stay listed in a muted section for 5 minutes.

Game should be in windowed / borderless windowed mode for the overlay to sit on top cleanly.

## Duration model

- 1 tick = 6 seconds
- Base ticks come from `data/spells.json` (`fixed` or simple level formulas)
- Tier bonus: `round(ticks × (1 + tier × tier_duration_pct/100))`
- Timers start on **land**, not begin-cast (avoids fizzle/interrupt false positives)

Edit `data/spells.json` to add spells (name, land/wear messages, ticks, category, optional `classes: [{ class, level }]`). The settings UI groups watched spells by **class → level**; multi-class spells appear under every applicable class. Spells without class data land under **Other**.

Wiki scrape (`node scripts/scrape-eql-wiki.mjs`) writes `data/spells.wiki.json` (rich) and regenerates shipped `data/spells.json` including `classes`, preserving local message/`watched_by_default` corrections when present.

## Development

### Requirements

- Windows 10/11
- [Node.js](https://nodejs.org/) LTS
- [Rust](https://rustup.rs/) (stable)
- WebView2
- Visual Studio Build Tools with the **Desktop development with C++** workload (for compiling the Tauri backend)

### Setup

```bash
npm install
npm run tauri dev
```

```bash
# Frontend only
npm run dev

# Full app (settings + overlay)
npm run tauri dev

# Rust unit tests (parser, duration, fixture mez cast/land/break)
npm run test:rust
```

### Local release builds

```bash
# NSIS installer → src-tauri/target/release/bundle/nsis/
npm run release:installer

# Portable copy → app/Berryworks.exe (+ resources)
npm run release:app

# Both installer + portable copy
npm run release
```

### Cutting a GitHub Release

1. Bump `version` in `package.json` and `src-tauri/tauri.conf.json` (and `src-tauri/Cargo.toml` if you keep them aligned).
2. Commit, then tag and push:

```bash
git tag v0.1.0
git push origin v0.1.0
```

3. The [Release workflow](.github/workflows/release.yml) builds on `windows-latest`, signs updater artifacts, and uploads the NSIS installer plus `latest.json` to a GitHub Release for that tag. You can also run the workflow manually via **Actions → Release → Run workflow**.

### In-app updates

Installed builds can check **Preferences → Updates** (also checks quietly a few seconds after launch). Updates are verified with a Tauri signing keypair.

One-time setup (maintainers):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup-updater-keys.ps1 -SetGithubSecrets
```

That writes the **public** key into `src-tauri/tauri.conf.json` and stores the **private** key in GitHub Actions secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Back up the private key; losing it means existing installs cannot verify future updates.

Local release builds that produce `.sig` files also need those env vars set in your shell before `npm run release:installer`.

Validation fixture: `fixtures/sample_mez.log` covers mez land → awaken break, Clarity self-buff, Root land, interrupt/fizzle discarded, Entrance land.

## Project layout

```
berry-timers/
  data/spells.json     # curated spell definitions
  src/                 # settings + overlay UI
  src-tauri/           # log tailer, parser, timer engine
  fixtures/            # sample log snippets for validation
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

MIT — see [LICENSE](LICENSE).
