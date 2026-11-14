//! A small diagnostic log an app can attach to a bug report.
//!
//! Append-only text, one line per event: `<local time with offset> <tag> <message>`. The log is
//! two files of at most [`MAX_BYTES`] each (`diag.log` and `diag.1.log`, in `<files>/sentinel/`):
//! when the newest is full it becomes the older one and the previous older one is dropped, so the
//! log never grows past twice the limit. Nothing leaves the phone by itself: the app decides what
//! to do with [`read`], for example mail it after the user pressed send.
//!
//! Panics are written here too (tag `panic`, see `init`), next to the file `init::PANIC_LOG_FILE`.
//! Log events, never content the user typed: that is the app's rule to keep.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// One file of the log holds at most this much.
pub const MAX_BYTES: u64 = 256 * 1024;

/// Where the log lives under the app files directory.
pub const LOG_DIR: &str = "sentinel";

const CURRENT: &str = "diag.log";
const PREVIOUS: &str = "diag.1.log";

/// A rotating log in one directory. [`log`] uses one in the app's files directory; this is the
/// type itself, for tests and for apps that want another place.
pub struct DiagLog {
    current: PathBuf,
    previous: PathBuf,
    max_bytes: u64,
    lock: Mutex<()>,
}

impl DiagLog {
    pub fn new(dir: &Path, max_bytes: u64) -> Self {
        Self {
            current: dir.join(CURRENT),
            previous: dir.join(PREVIOUS),
            max_bytes,
            lock: Mutex::new(()),
        }
    }

    /// Append one line (a newline inside it becomes ` / `). Best effort: a failing disk must not
    /// bring the app down.
    pub fn append(&self, line: &str) {
        let Ok(_guard) = self.lock.lock() else {
            return;
        };
        let line = one_line(line);
        if let Some(dir) = self.current.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let size = std::fs::metadata(&self.current)
            .map(|m| m.len())
            .unwrap_or(0);
        if size > 0 && size + line.len() as u64 + 1 > self.max_bytes {
            let _ = std::fs::remove_file(&self.previous);
            let _ = std::fs::rename(&self.current, &self.previous);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.current)
        {
            let _ = writeln!(file, "{line}");
        }
    }

    /// The whole log, oldest line first.
    pub fn read_all(&self) -> String {
        let Ok(_guard) = self.lock.lock() else {
            return String::new();
        };
        let mut text = std::fs::read_to_string(&self.previous).unwrap_or_default();
        text.push_str(&std::fs::read_to_string(&self.current).unwrap_or_default());
        text
    }

    /// Remove both files.
    pub fn clear(&self) {
        if let Ok(_guard) = self.lock.lock() {
            let _ = std::fs::remove_file(&self.previous);
            let _ = std::fs::remove_file(&self.current);
        }
    }
}

fn one_line(text: &str) -> String {
    text.trim_end()
        .replace("\r\n", " / ")
        .replace(['\n', '\r'], " / ")
}

/// One log line.
pub fn format_line(timestamp: &str, tag: &str, message: &str) -> String {
    format!("{timestamp} {tag} {message}")
}

fn global() -> Option<&'static DiagLog> {
    static LOG: OnceLock<Option<DiagLog>> = OnceLock::new();
    LOG.get_or_init(|| {
        let files = crate::app_files_dir();
        (files.as_path() != Path::new(".")).then(|| DiagLog::new(&files.join(LOG_DIR), MAX_BYTES))
    })
    .as_ref()
}

/// Add an event to the app's log. Does nothing where there is no app files directory (a host
/// build, or before the app context is there).
pub fn log(tag: &str, message: &str) {
    if let Some(log) = global() {
        log.append(&format_line(
            &chrono::Local::now()
                .format("%Y-%m-%dT%H:%M:%S%.3f%:z")
                .to_string(),
            tag,
            message,
        ));
    }
}

/// The app's whole log, oldest line first.
pub fn read() -> String {
    global().map(DiagLog::read_all).unwrap_or_default()
}

/// Remove the app's log.
pub fn clear() {
    if let Some(log) = global() {
        log.clear();
    }
}

/// The last `max_lines` lines of `text`.
pub fn tail(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(max_lines);
    let mut out = lines[from..].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Lines of `text` that are at `timestamp` or later (same format as [`log`] writes, which sorts
/// as text within one time zone offset; across an offset change a line may be off by the offset).
pub fn lines_since<'a>(text: &'a str, timestamp: &str) -> Vec<&'a str> {
    text.lines()
        .filter(|line| line.get(..timestamp.len()).is_some_and(|t| t >= timestamp))
        .collect()
}

/// What kind of phone this is, for the top of a bug report: maker and model, Android version,
/// build, language and time zone. No names, no identifiers of the phone or the user.
pub fn device_summary() -> String {
    let now = chrono::Local::now();
    let mut out = String::new();
    #[cfg(target_os = "android")]
    {
        let prop = |key: &str| getprop(key);
        out.push_str(&format!(
            "Device: {} {}\nAndroid: {} (API {})\nBuild: {}\n",
            prop("ro.product.manufacturer"),
            prop("ro.product.model"),
            prop("ro.build.version.release"),
            prop("ro.build.version.sdk"),
            prop("ro.build.display.id"),
        ));
        out.push_str(&format!(
            "Language: {}\nTime zone: {} (UTC{})\n",
            crate::device_language(),
            prop("persist.sys.timezone"),
            now.format("%:z")
        ));
    }
    #[cfg(not(target_os = "android"))]
    {
        out.push_str(&format!(
            "Device: {} {}\nLanguage: {}\nTime zone: UTC{}\n",
            std::env::consts::OS,
            std::env::consts::ARCH,
            crate::device_language(),
            now.format("%:z")
        ));
    }
    out
}

#[cfg(target_os = "android")]
fn getprop(key: &str) -> String {
    std::process::Command::new("getprop")
        .arg(key)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "?".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_kept_in_order_and_newlines_cannot_split_one() {
        let dir = tempfile::tempdir().unwrap();
        let log = DiagLog::new(dir.path(), 1000);
        log.append("one");
        log.append("two\nstill two");
        assert_eq!(log.read_all(), "one\ntwo / still two\n");
    }

    #[test]
    fn a_full_file_becomes_the_older_one_and_the_oldest_goes() {
        let dir = tempfile::tempdir().unwrap();
        let log = DiagLog::new(dir.path(), 100);
        let line = "x".repeat(49);
        // 50 bytes per line with its newline: two fit.
        for n in 0..6 {
            log.append(&format!("{n}{}", &line[1..]));
        }
        let text = log.read_all();
        let first: Vec<&str> = text.lines().map(|l| &l[..1]).collect();
        assert_eq!(first, ["2", "3", "4", "5"]);
        let total = std::fs::metadata(dir.path().join(CURRENT)).unwrap().len()
            + std::fs::metadata(dir.path().join(PREVIOUS)).unwrap().len();
        assert!(total <= 200);
    }

    #[test]
    fn the_log_never_grows_past_twice_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let log = DiagLog::new(dir.path(), 1024);
        for n in 0..500 {
            log.append(&format!("line {n} with some words in it"));
        }
        assert!(log.read_all().len() <= 2048);
        assert!(log.read_all().ends_with("line 499 with some words in it\n"));
    }

    #[test]
    fn clear_removes_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let log = DiagLog::new(dir.path(), 60);
        for _ in 0..5 {
            log.append("a line of text that is long enough");
        }
        log.clear();
        assert_eq!(log.read_all(), "");
    }

    #[test]
    fn tail_and_since_pick_lines() {
        let text = "2026-11-14T08:00:00.000+01:00 a x\n2026-11-14T09:00:00.000+01:00 b y\n2026-11-14T10:00:00.000+01:00 c z\n";
        assert_eq!(tail(text, 2).lines().count(), 2);
        assert!(tail(text, 2).starts_with("2026-11-14T09"));
        assert_eq!(lines_since(text, "2026-11-14T09:30").len(), 1);
        assert_eq!(format_line("T", "ring", "started"), "T ring started");
    }

    #[test]
    fn the_device_summary_names_language_and_time_zone() {
        let s = device_summary();
        assert!(s.contains("Language:"));
        assert!(s.contains("Time zone:"));
    }
}
