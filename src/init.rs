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
    /// Log tag for Android logcat. Default: "MobileSentinel".
    pub log_tag: String,
    /// Maximum log level. Default: Debug.
    pub log_level: log::LevelFilter,
}

impl Default for InitConfig {
    fn default() -> Self {
        Self {
            logger: true,
            crash_loop_mitigation: true,
            log_tag: "MobileSentinel".to_string(),
            log_level: log::LevelFilter::Debug,
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
        assert_eq!(config.log_tag, "MobileSentinel");
        assert_eq!(config.log_level, log::LevelFilter::Debug);
    }
}
