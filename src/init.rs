//! One-liner initialization for mobile-sentinel.
//!
//! Sets up logging and crash-loop mitigation. Platform capabilities are
//! reached through their feature-gated modules; the firing surface is
//! reached through the [`crate::FiringSink`] installed via
//! [`crate::install_firing_sink`] (on Android the consumer constructs
//! `AndroidFiringSink`). There is no central backend container.

/// Configuration for mobile-sentinel initialization.
#[derive(Debug, Clone)]
pub struct InitConfig {
    /// Initialize Android logger (logcat). Default: true.
    pub logger: bool,
    /// Install SIGABRT handler to prevent crash-loop throttling. Default: true.
    pub crash_loop_mitigation: bool,
    /// End the process with `_exit` when the UI framework calls `exit()`,
    /// skipping C/C++ static destructors. Default: true. See
    /// `install_clean_process_exit`.
    pub clean_process_exit: bool,
    /// Log every Rust panic (any thread) to logcat and append it to
    /// `<files>/sentinel/last_panic.txt`. Default: true. Without it a panic
    /// on a worker or UI thread is invisible: the default hook writes to
    /// stderr, which Android discards, and crash-loop mitigation turns
    /// aborts into a clean exit.
    pub panic_log: bool,
    /// Log tag for Android logcat. Default: "MobileSentinel".
    pub log_tag: String,
    /// Maximum log level. Default: Debug in debug builds, Info in release.
    pub log_level: log::LevelFilter,
}

impl Default for InitConfig {
    fn default() -> Self {
        Self {
            logger: true,
            crash_loop_mitigation: true,
            clean_process_exit: true,
            panic_log: true,
            log_tag: "MobileSentinel".to_string(),
            // Verbose in debug builds; release keeps logcat (and the work of
            // formatting log lines) to what matters.
            log_level: if cfg!(debug_assertions) {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        }
    }
}

/// Initialize mobile-sentinel: logcat logger + SIGABRT crash-loop
/// mitigation on Android. A no-op on other platforms beyond consuming the
/// config. Platform capabilities are accessed via their feature-gated
/// modules, not a return value.
pub fn init(config: InitConfig) {
    #[cfg(target_os = "android")]
    {
        if config.logger {
            android_logger::init_once(
                android_logger::Config::default()
                    .with_max_level(config.log_level)
                    .with_tag(config.log_tag.as_bytes().to_vec()),
            );
        }
        if config.crash_loop_mitigation {
            install_sigabrt_handler();
        }
        if config.clean_process_exit {
            install_clean_process_exit();
        }
        if config.panic_log {
            install_panic_log();
        }
        log::info!("[mobile-sentinel] init complete");
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = config; // suppress unused warning on host builds
    }
}

/// Install a SIGABRT handler that calls `_exit(0)` instead of crashing.
/// Prevents Android's crash-loop throttle from blocking `:sentinel`
/// process resurrection.
#[cfg(target_os = "android")]
fn install_sigabrt_handler() {
    extern "C" fn clean_exit_handler(_sig: libc::c_int) {
        unsafe {
            libc::_exit(0);
        }
    }
    unsafe {
        libc::signal(
            libc::SIGABRT,
            clean_exit_handler as *const () as libc::sighandler_t,
        );
    }
    log::info!("[mobile-sentinel] SIGABRT handler installed (crash-loop mitigation)");
}

/// Make a framework-initiated `exit()` end the process immediately.
///
/// On Android, `tao` (the windowing layer under Dioxus/Tauri) ends its event
/// loop with `std::process::exit()` when the activity is destroyed. `exit()`
/// runs every library's static destructors while JNI/WebView/worker threads
/// are still alive; one of them then locks a just-destroyed mutex and libc
/// aborts ("FORTIFY: pthread_mutex_lock called on a destroyed mutex"). The
/// process is ending anyway, so an `atexit` handler that calls `_exit(0)` —
/// handlers run newest-first, before destructors registered at library load —
/// ends it cleanly instead of as a native crash.
#[cfg(target_os = "android")]
fn install_clean_process_exit() {
    extern "C" fn fast_exit() {
        unsafe { libc::_exit(0) }
    }
    let rc = unsafe { libc::atexit(fast_exit) };
    if rc == 0 {
        log::info!("[mobile-sentinel] clean process exit installed");
    } else {
        log::warn!("[mobile-sentinel] atexit registration failed (rc={rc})");
    }
}

/// File (under the app files dir) that collects panic reports.
pub const PANIC_LOG_FILE: &str = "sentinel/last_panic.txt";

/// Panic reports are appended until the file reaches this size, then the
/// next report starts it over.
const PANIC_LOG_MAX_BYTES: u64 = 64 * 1024;

/// Install a panic hook that reports every panic to logcat and to
/// [`PANIC_LOG_FILE`], then runs the previous hook. Installed at most once.
///
/// The file path is resolved here, not in the hook: looking it up can need
/// JNI, which is not something to do on a panicking thread.
#[cfg(target_os = "android")]
fn install_panic_log() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let files = crate::app_files_dir();
        let log_path =
            (files.as_path() != std::path::Path::new(".")).then(|| files.join(PANIC_LOG_FILE));
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "<unknown>".to_owned());
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string payload>".to_owned());
            let report = format_panic_report(
                &chrono::Utc::now().to_rfc3339(),
                thread.name().unwrap_or("<unnamed>"),
                &location,
                &message,
            );
            log::error!("[mobile-sentinel] {report}");
            if let Some(path) = &log_path {
                append_panic_report(path, &report);
            }
            previous(info);
        }));
        log::info!("[mobile-sentinel] panic log installed");
    });
}

/// One panic report line: when, which thread, where, and the message.
pub fn format_panic_report(timestamp: &str, thread: &str, location: &str, message: &str) -> String {
    format!("PANIC {timestamp} thread '{thread}' at {location}: {message}")
}

/// Append `report` to the panic log, starting the file over once it is
/// larger than [`PANIC_LOG_MAX_BYTES`]. Best-effort: errors are ignored.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn append_panic_report(path: &std::path::Path, report: &str) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let too_big = std::fs::metadata(path)
        .map(|m| m.len() > PANIC_LOG_MAX_BYTES)
        .unwrap_or(false);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(!too_big)
        .truncate(too_big)
        .open(path);
    if let Ok(mut f) = file {
        let _ = writeln!(f, "{report}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_report_names_thread_location_and_message() {
        let r = format_panic_report(
            "2026-10-24T07:40:00+00:00",
            "tokio-runtime-worker",
            "src/app.rs:42",
            "index out of bounds",
        );
        assert_eq!(
            r,
            "PANIC 2026-10-24T07:40:00+00:00 thread 'tokio-runtime-worker' at src/app.rs:42: index out of bounds"
        );
    }

    #[test]
    fn panic_reports_append_then_start_over_when_too_big() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sentinel/last_panic.txt");
        append_panic_report(&path, "first");
        append_panic_report(&path, "second");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first\nsecond\n");

        std::fs::write(&path, "x".repeat(PANIC_LOG_MAX_BYTES as usize + 1)).unwrap();
        append_panic_report(&path, "fresh");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh\n");
    }

    #[test]
    fn init_with_default_config_does_not_panic() {
        init(InitConfig::default());
    }

    #[test]
    fn init_config_default_values() {
        let config = InitConfig::default();
        assert!(config.logger);
        assert!(config.crash_loop_mitigation);
        assert!(config.clean_process_exit);
        assert!(config.panic_log);
        assert_eq!(config.log_tag, "MobileSentinel");
        let expected = if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        };
        assert_eq!(config.log_level, expected);
    }
}
