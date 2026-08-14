# Changelog

All notable changes to [Berryworks](https://github.com/jungleberrydev/berryworks) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The in-app **What's new** dialog and Preferences → Updates list are generated from this file.

## [Unreleased]

### Added

- Combat meter: live group/raid DPS from the character log, ability breakdown, optional always-on-top overlay, and a session tracker (kills, plat, /hr rates).
- Unlocked overlays show a small Timers / Enemies / Respawns / Alerts / DPS label so windows are easy to tell apart.
- Header **Lock Overlays** / **Unlock Overlays** button (same as Preferences → General).

### Fixed

- Charm pet DPS no longer drops mid-fight when a different NPC of the same name dies, when a group buff lands during your charm cast, or when that name's damage shield hits you. The own-cast bind window uses log timestamps so a delayed log tail still counts.
- DPS overlay can be dragged and shows its **DPS** label when unlocked (the window was missing from Tauri permissions).

### Changed

- Charm pets for the meter (and overlay charm timers) bind only when your own recent cast resolved the broadcast, stop crediting if that mob hits you, and drop on zone. Nearby enchanters' charms are no longer treated as yours.
- Main window uses a left rail (Combat, Loot, Timers, Respawns) with Preferences as its own page. Overlay, appearance, watched spells, loot sync, and updates live there instead of nested tabs on every feature page.
- Search fields and checkboxes follow the active theme instead of native light Windows controls.
- DPS overlay toggle lives on Combat. Timer overlay settings stay under Preferences → Overlay. The header Show Overlay button is gone; the timer overlay stays visible, and other overlays follow their section toggles.

## [0.2.5] - 2026-08-13

### Added

- Voice and overlay alert when your charm breaks (`Your Allure spell has worn off of a gnoll`, `Your charm spell has worn off`, and the same pattern for other charm names).
- Voice and overlay alert when invis starts fading (`You feel yourself starting to appear`) or drops (`You appear`, plus Camouflage / Gather Shadows / IVU / IVA wear-off lines).
- Positionable **alert overlay** for fading on-screen messages. Unlock to drag it; size, font, and charm/invis colors are configurable. Messages fade after a set duration.

## [0.2.4] - 2026-08-12

### Added

- Header Updates controls (version, Check for updates, What's new) and an in-app update dialog instead of a browser confirm.

### Fixed

- Overlay windows restore last position and size after lock, show, and DWM chrome races.
- NPC self-buffs (Cleric of Innoruuk, spite golem, unnamed a/an/the targets) no longer start Swift Like the Wind or Spirit of Wolf timers, so death no longer announces a false wear-off. Overlay voice matches visible rows.
- Wear-off matching uses the longest phrase so a slow's "Your speed returns" does not clear haste.

## [0.2.3] - 2026-08-12

### Added

- In-app changelog: What's new dialog after you update, and a full history on General → Updates.
- Spell Casting Reinforcement AA rank setting for beneficial buff timers.

### Fixed

- Long buff durations that listed hours on the wiki (Armor of Faith, Aegis, Shield of Words, and 50 similar spells) were stored as minutes only. Timers now use the client duration cap.
- Overlay timer rows no longer strobe while flashing for expiry.
- Expiry warning lead time can be typed in seconds.

## [0.2.2] - 2026-08-12

### Added

- Observation-based loot drop rates on the Loot tab.
- Configurable timer expiry warnings (flash and verbal) with a shared lead time.

## [0.2.1] - 2026-08-12

### Added

- In-app updater that checks GitHub Releases and can download the installer.

### Fixed

- Restored short-duration HoT spell data.

## [0.2.0] - 2026-08-12

### Added

- Local loot tracking and Norrath Roster community sync.
- Discord login for loot uploads.
- Announcement voice volume.
- Spirit of the Puma spell data.

## [0.1.0] - 2026-08-12

Initial public release of Berryworks — an EverQuest Legends timers toolkit that tails your character log, tracks buff/debuff and rare respawn timers, and shows them on always-on-top overlays.

### Added

- EverQuest Legends log-tail timers: land-based spell tracking with target names, tick/tier duration model, and movable always-on-top overlays (main, optional enemies, respawns).
- Settings UI with tabs: **General**, **Appearance**, **Overlay**, **Respawns**, and **Spells** (class → level watched-spell groups).
- Product rename to **Berryworks** in the UI and installer (`productName` / window titles); bundle identifier remains `com.evans.berry-timers`.
- Voice announcements when buffs wear off or are dismissed, with announcement voice picker and **Test voice** (Settings → Overlay).
- Character log auto-detect on the General tab via `suggest_log_paths` (common EverQuest / Daybreak install paths), plus Browse.
- App icon: berry bush on dark `#160e14`.
- Windows NSIS installer bundle and GitHub Actions release workflow (tag-driven builds uploading `Berryworks_*_x64-setup.exe`).
- MIT license and README install / in-game / development docs.

[Unreleased]: https://github.com/jungleberrydev/berryworks/compare/v0.2.5...HEAD
[0.2.5]: https://github.com/jungleberrydev/berryworks/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/jungleberrydev/berryworks/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/jungleberrydev/berryworks/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/jungleberrydev/berryworks/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/jungleberrydev/berryworks/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/jungleberrydev/berryworks/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jungleberrydev/berryworks/releases/tag/v0.1.0
