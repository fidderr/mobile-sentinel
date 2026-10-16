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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(config.log_tag, "MobileSentinel");
        let expected = if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        };
        assert_eq!(config.log_level, expected);
    }
}
