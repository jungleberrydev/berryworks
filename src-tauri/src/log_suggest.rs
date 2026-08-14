//! Suggest common EverQuest / EQL character log paths on first run.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SuggestedLog {
    pub path: String,
    pub label: String,
}

/// Scan well-known EQ / EQL Logs folders for `eqlog_*.txt`.
pub fn suggest_log_paths() -> Vec<SuggestedLog> {
    let mut seen = HashSet::new();
    let mut found: Vec<(PathBuf, SystemTime)> = Vec::new();

    for dir in candidate_log_dirs() {
        collect_eq_logs(&dir, &mut seen, &mut found);
    }

    found.sort_by(|a, b| b.1.cmp(&a.1));
    found
        .into_iter()
        .map(|(path, _)| SuggestedLog {
            label: label_for(&path),
            path: path.to_string_lossy().into_owned(),
        })
        .collect()
}

fn candidate_log_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(public) = std::env::var_os("PUBLIC").map(PathBuf::from) {
        dirs.push(
            public
                .join("Daybreak Game Company")
                .join("Installed Games")
                .join("EverQuest Legends")
                .join("Logs"),
        );
        dirs.push(
            public
                .join("Daybreak Game Company")
                .join("Installed Games")
                .join("EverQuest")
                .join("Logs"),
        );
        dirs.push(
            public
                .join("Sony Online Entertainment")
                .join("Installed Games")
                .join("EverQuest")
                .join("Logs"),
        );
    }

    if let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    {
        let documents = home.join("Documents");
        for name in [
            "EverQuest Legends",
            "EverQuest",
            "EQ",
            "Daybreak Game Company/EverQuest Legends",
            "Daybreak Game Company/EverQuest",
        ] {
            dirs.push(documents.join(name).join("Logs"));
        }
    }

    if let Some(pf86) = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from) {
        dirs.push(
            pf86.join("Steam")
                .join("steamapps")
                .join("common")
                .join("EverQuest")
                .join("Logs"),
        );
        dirs.push(
            pf86.join("Steam")
                .join("steamapps")
                .join("common")
                .join("EverQuest Legends")
                .join("Logs"),
        );
        dirs.push(
            pf86.join("Daybreak Game Company")
                .join("EverQuest")
                .join("Logs"),
        );
    }

    if let Some(pf) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
        dirs.push(
            pf.join("Steam")
                .join("steamapps")
                .join("common")
                .join("EverQuest")
                .join("Logs"),
        );
        dirs.push(
            pf.join("Daybreak Game Company")
                .join("EverQuest Legends")
                .join("Logs"),
        );
    }

    dirs
}

fn collect_eq_logs(dir: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<(PathBuf, SystemTime)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if !(lower.starts_with("eqlog_") && lower.ends_with(".txt")) {
            continue;
        }
        let Ok(canon) = path.canonicalize() else {
            // Still offer the path if canonicalize fails (permissions / missing drive).
            if seen.insert(path.clone()) {
                let modified = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                out.push((path, modified));
            }
            continue;
        };
        if !seen.insert(canon.clone()) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        out.push((canon, modified));
    }
}

/// Character name from `eqlog_Character_Server.txt`, or empty if unknown.
pub fn character_name_from_log_path(path: &str) -> String {
    let file = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let stem = file.trim_end_matches(".txt").trim_end_matches(".TXT");
    let rest = stem.strip_prefix("eqlog_").unwrap_or(stem);
    if let Some((character, server)) = rest.rsplit_once('_') {
        if !character.is_empty() && !server.is_empty() {
            return character.to_string();
        }
    }
    String::new()
}

fn label_for(path: &Path) -> String {
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("eqlog");
    // eqlog_Character_Server.txt → Character (Server)
    let stem = file.trim_end_matches(".txt").trim_end_matches(".TXT");
    let rest = stem.strip_prefix("eqlog_").unwrap_or(stem);
    if let Some((character, server)) = rest.rsplit_once('_') {
        if !character.is_empty() && !server.is_empty() {
            return format!("{character} ({server})");
        }
    }
    rest.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn label_parses_character_and_server() {
        let path = PathBuf::from(r"C:\Games\Logs\eqlog_Jungleberry_Povar.txt");
        assert_eq!(label_for(&path), "Jungleberry (Povar)");
        assert_eq!(
            character_name_from_log_path(r"C:\Games\Logs\eqlog_Jungleberry_Povar.txt"),
            "Jungleberry"
        );
    }

    #[test]
    fn collect_finds_eqlog_files() {
        let dir = std::env::temp_dir().join(format!(
            "berryworks-log-suggest-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("eqlog_Test_Server.txt");
        File::create(&log)
            .unwrap()
            .write_all(b"test")
            .unwrap();
        File::create(dir.join("notes.txt"))
            .unwrap()
            .write_all(b"skip")
            .unwrap();

        let mut seen = HashSet::new();
        let mut found = Vec::new();
        collect_eq_logs(&dir, &mut seen, &mut found);
        assert_eq!(found.len(), 1);
        assert!(found[0]
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .eq_ignore_ascii_case("eqlog_Test_Server.txt"));

        let _ = fs::remove_dir_all(&dir);
    }
}
