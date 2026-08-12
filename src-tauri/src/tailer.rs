use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

pub enum TailCommand {
    SetPath(PathBuf),
    #[allow(dead_code)]
    Stop,
}

/// Spawns a background thread that tails a log file and sends new lines on `line_tx`.
pub fn start_tailer(line_tx: Sender<String>) -> Sender<TailCommand> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<TailCommand>();

    thread::spawn(move || {
        let mut current_path: Option<PathBuf> = None;
        let mut offset: u64 = 0;
        let mut _watcher: Option<RecommendedWatcher> = None;
        let (fs_tx, fs_rx) = mpsc::channel();

        loop {
            // Non-blocking command poll with short timeout so we also poll the file
            match cmd_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(TailCommand::SetPath(path)) => {
                    current_path = Some(path.clone());
                    offset = file_len(&path).unwrap_or(0);
                    _watcher = make_watcher(&path, fs_tx.clone()).ok();
                }
                Ok(TailCommand::Stop) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            // Drain filesystem events
            while fs_rx.try_recv().is_ok() {}

            if let Some(path) = current_path.clone() {
                if let Ok(new_offset) = read_new_lines(&path, offset, &line_tx) {
                    // Handle truncation / rotation
                    let len = file_len(&path).unwrap_or(new_offset);
                    offset = if len < offset { 0 } else { new_offset };
                    if len < offset {
                        let _ = read_new_lines(&path, 0, &line_tx);
                    }
                }
            }
        }
    });

    cmd_tx
}

fn make_watcher(
    path: &Path,
    fs_tx: Sender<notify::Result<notify::Event>>,
) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(fs_tx)?;
    let watch_path = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(path);
    watcher.watch(watch_path, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

fn file_len(path: &Path) -> std::io::Result<u64> {
    Ok(std::fs::metadata(path)?.len())
}

fn read_new_lines(path: &Path, mut offset: u64, line_tx: &Sender<String>) -> std::io::Result<u64> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len < offset {
        offset = 0;
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            break;
        }
        // Incomplete line (no newline yet) — wait for more data
        if !buf.ends_with('\n') {
            break;
        }
        offset += n as u64;
        let line = buf.trim_end_matches(['\r', '\n']).to_string();
        if !line.is_empty() {
            let _ = line_tx.send(line);
        }
    }
    Ok(offset)
}

/// Shared helper used by tests / injection
#[allow(dead_code)]
pub fn process_file_from_start(path: &Path) -> std::io::Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    Ok(reader
        .lines()
        .filter_map(|l| l.ok())
        .filter(|l| !l.is_empty())
        .collect())
}
