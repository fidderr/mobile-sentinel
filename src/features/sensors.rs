//! Sensors capability — accelerometer-backed shake and squat counters, a raw
//! accelerometer recorder, and the hardware step counter.
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
//! is enabled (see `build::registry` (feature `build`)).

#[cfg(any(target_os = "android", test))]
pub(crate) mod recorder;
pub mod squat;

pub use squat::{SquatConfig, SquatDetector};

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

/// Start the accelerometer and count squats (repetitions of slowly lowering and raising the
/// phone) from it, with the sensor's own timestamps. See [`SquatDetector`] for how, and what it
/// cannot tell apart. Starting again resets the count.
pub fn start_squat_counter() {
    #[cfg(target_os = "android")]
    crate::platform::android::sensor_state::start_squat_counter();
}

/// Stop the squat counter and the accelerometer.
pub fn stop_squat_counter() {
    #[cfg(target_os = "android")]
    crate::platform::android::sensor_state::stop_squat_counter();
}

/// Squats counted since the counter started or was reset.
pub fn squat_count() -> i32 {
    #[cfg(target_os = "android")]
    {
        crate::platform::android::sensor_state::get_squat_count()
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// Reset the squat counter to zero.
pub fn reset_squat_count() {
    #[cfg(target_os = "android")]
    crate::platform::android::sensor_state::reset_squat_count();
}

/// Record every raw accelerometer sample to a CSV file (`t_ns,x,y,z`, sensor time in
/// nanoseconds, m/s^2), for tuning detectors on real movements. The accelerometer has to be
/// running (`start_accelerometer` or `start_squat_counter`). Returns false if the file cannot be
/// created. For debugging: it writes at the sensor rate until [`stop_recording`].
pub fn start_recording(path: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        crate::platform::android::sensor_state::start_recording(path)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = path;
        false
    }
}

/// Stop recording and close the file. Returns the number of samples written.
pub fn stop_recording() -> u64 {
    #[cfg(target_os = "android")]
    {
        crate::platform::android::sensor_state::stop_recording()
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
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
