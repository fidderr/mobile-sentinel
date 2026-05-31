//! Sensors capability — accelerometer-backed shake counter and hardware
//! step counter.
//!
//! Gated behind the `sensors` Cargo feature. The sole entry point is this
//! module, so the gate can't be bypassed.
//!
//! ```toml
//! mobile-sentinel = { version = "...", features = ["sensors"] }
//! ```
//!
//! The step counter additionally requires the `ACTIVITY_RECOGNITION`
//! permission, which `build_sentinel` injects when the `sensors` capability
//! is enabled (see [`crate::build::registry`]).

/// Start the accelerometer-backed shake detector.
pub fn start_accelerometer() {
    #[cfg(target_os = "android")]
    crate::platform::android::sensor_state::start_accelerometer();
}

/// Stop the accelerometer-backed shake detector.
pub fn stop_accelerometer() {
    #[cfg(target_os = "android")]
    crate::platform::android::sensor_state::stop_accelerometer();
}

/// Shake count accumulated since the last reset.
pub fn shake_count() -> i32 {
    #[cfg(target_os = "android")]
    {
        crate::platform::android::sensor_state::get_shake_count()
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// Reset the shake counter to zero.
pub fn reset_shake_count() {
    #[cfg(target_os = "android")]
    crate::platform::android::sensor_state::reset_shake_count();
}

/// Start the hardware step counter.
pub fn start_step_counter() {
    #[cfg(target_os = "android")]
    crate::platform::android::sensor_state::start_step_counter();
}

/// Stop the hardware step counter.
pub fn stop_step_counter() {
    #[cfg(target_os = "android")]
    crate::platform::android::sensor_state::stop_step_counter();
}

/// Steps counted since the counter started.
pub fn step_count() -> i32 {
    #[cfg(target_os = "android")]
    {
        crate::platform::android::sensor_state::get_step_count()
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}
