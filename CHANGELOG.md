# Changelog

All notable changes to [Berryworks](https://github.com/jungleberrydev/berryworks) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/jungleberrydev/berryworks/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jungleberrydev/berryworks/releases/tag/v0.1.0
