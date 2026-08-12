# Changelog

All notable changes to [Berryworks](https://github.com/jungleberrydev/berryworks) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The in-app **What's new** dialog and General → Updates list are generated from this file.

## [Unreleased]

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

[Unreleased]: https://github.com/jungleberrydev/berryworks/compare/v0.2.3...HEAD
[0.2.3]: https://github.com/jungleberrydev/berryworks/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/jungleberrydev/berryworks/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/jungleberrydev/berryworks/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/jungleberrydev/berryworks/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jungleberrydev/berryworks/releases/tag/v0.1.0
