//! Global sensor state for accelerometer and step counter.
//! Kotlin delivers sensor events via JNI → these are stored here.
//! Consumer UI polls the values via the public API.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use once_cell::sync::Lazy;

/// Global sensor state — written by JNI callbacks, read by consumer UI.
pub struct SensorState {
    /// Current shake count (detected from accelerometer magnitude spikes).
    pub shake_count: AtomicI32,
    /// Current step count (relative to when step counter was started).
    pub step_count: AtomicI32,
    /// Whether accelerometer is currently active.
    pub accel_active: AtomicBool,
    /// Whether step counter is currently active.
    pub steps_active: AtomicBool,
}

static STATE: Lazy<SensorState> = Lazy::new(|| SensorState {
    shake_count: AtomicI32::new(0),
    step_count: AtomicI32::new(0),
    accel_active: AtomicBool::new(false),
    steps_active: AtomicBool::new(false),
});

/// Reset shake count to zero (called when starting a new shake challenge).
pub fn reset_shake_count() {
    STATE.shake_count.store(0, Ordering::Relaxed);
}

/// Get current shake count.
pub fn get_shake_count() -> i32 {
    STATE.shake_count.load(Ordering::Relaxed)
}

/// Get current step count.
pub fn get_step_count() -> i32 {
    STATE.step_count.load(Ordering::Relaxed)
}

/// Start the accelerometer for shake detection.
/// Calls the Kotlin SentinelSensorHelper.startAccelerometer() via JNI.
#[cfg(target_os = "android")]
pub fn start_accelerometer() {
    use crate::platform::android::callbacks::load_app_class_global;

    STATE.shake_count.store(0, Ordering::Relaxed);
    STATE.accel_active.store(true, Ordering::Relaxed);

    let helper_ref = match load_app_class_global("com/mobilesentinel/SentinelSensorHelper") {
        Some(r) => r,
        None => {
            log::error!("[sensor_state] Failed to load SentinelSensorHelper");
            return;
        }
    };

    let (vm, _activity) = match crate::platform::android::context::get_activity() {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return,
    };

    let cls = unsafe { jni::objects::JClass::from_raw(helper_ref.as_raw()) };
    let _ = env.call_static_method(&cls, "startAccelerometer", "()Z", &[]);
    let _ = env.exception_clear();
    log::info!("[sensor_state] Accelerometer started for shake detection");
}

/// Stop the accelerometer.
#[cfg(target_os = "android")]
pub fn stop_accelerometer() {
    STATE.accel_active.store(false, Ordering::Relaxed);

    let helper_ref = match crate::platform::android::callbacks::load_app_class_global(
        "com/mobilesentinel/SentinelSensorHelper",
    ) {
        Some(r) => r,
        None => return,
    };

    let (vm, _activity) = match crate::platform::android::context::get_activity() {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return,
    };

    let cls = unsafe { jni::objects::JClass::from_raw(helper_ref.as_raw()) };
    let _ = env.call_static_method(&cls, "stopAccelerometer", "()V", &[]);
    let _ = env.exception_clear();
    log::info!("[sensor_state] Accelerometer stopped");
}

/// Start the step counter.
/// Permission should already be granted (requested when adding the challenge).
#[cfg(target_os = "android")]
pub fn start_step_counter() {
    STATE.step_count.store(0, Ordering::Relaxed);
    STATE.steps_active.store(true, Ordering::Relaxed);

    let helper_ref = match crate::platform::android::callbacks::load_app_class_global(
        "com/mobilesentinel/SentinelSensorHelper",
    ) {
        Some(r) => r,
        None => {
            log::error!("[sensor_state] Failed to load SentinelSensorHelper");
            return;
        }
    };

    let (vm, _activity) = match crate::platform::android::context::get_activity() {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return,
    };

    let cls = unsafe { jni::objects::JClass::from_raw(helper_ref.as_raw()) };
    let _ = env.call_static_method(&cls, "startStepCounter", "()Z", &[]);
    let _ = env.exception_clear();
    log::info!("[sensor_state] Step counter started");
}

/// Stop the step counter.
#[cfg(target_os = "android")]
pub fn stop_step_counter() {
    STATE.steps_active.store(false, Ordering::Relaxed);

    let helper_ref = match crate::platform::android::callbacks::load_app_class_global(
        "com/mobilesentinel/SentinelSensorHelper",
    ) {
        Some(r) => r,
        None => return,
    };

    let (vm, _activity) = match crate::platform::android::context::get_activity() {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return,
    };

    let cls = unsafe { jni::objects::JClass::from_raw(helper_ref.as_raw()) };
    let _ = env.call_static_method(&cls, "stopStepCounter", "()V", &[]);
    let _ = env.exception_clear();
    log::info!("[sensor_state] Step counter stopped");
}

// No-op stubs for non-Android
#[cfg(not(target_os = "android"))]
pub fn start_accelerometer() {}
#[cfg(not(target_os = "android"))]
pub fn stop_accelerometer() {}
#[cfg(not(target_os = "android"))]
pub fn start_step_counter() {}
#[cfg(not(target_os = "android"))]
pub fn stop_step_counter() {}

// ---------------------------------------------------------------------------
// Shake detection from accelerometer data
// ---------------------------------------------------------------------------

use std::sync::atomic::AtomicU64;

/// Approximate squared magnitude of gravity alone, in (m/s²)². Android's
/// accelerometer reports ~9.81 m/s² at rest, and 9.81² ≈ 96, so we subtract
/// this to get the motion-only component of the squared magnitude.
const GRAVITY_MAGNITUDE_SQ: f64 = 96.0;

/// Motion-only squared-magnitude threshold (in (m/s²)²) above which a sample
/// counts as a shake. ~20 (m/s²) of net motion → 20² = 400, i.e. roughly 2g
/// of shake force on top of gravity.
const SHAKE_THRESHOLD_SQ: f64 = 400.0;

/// Minimum time between counted shakes (ms) so one vigorous shake gesture
/// isn't counted many times across consecutive samples.
const SHAKE_COOLDOWN_MS: u64 = 300;

/// Timestamp (unix ms) of the last counted shake, for cooldown gating.
static LAST_SHAKE_TIME_MS: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

/// Called from JNI when accelerometer data arrives.
/// Detects shakes by checking if the motion-only magnitude exceeds a
/// threshold, gated by a cooldown so a single gesture counts once.
pub(crate) fn on_accelerometer_data(x: f64, y: f64, z: f64) {
    if !STATE.accel_active.load(Ordering::Relaxed) {
        return;
    }

    // Squared magnitude of the raw acceleration vector (avoid sqrt).
    let mag_sq = x * x + y * y + z * z;

    // Remove the gravity component to get motion-only squared magnitude.
    let accel_sq = (mag_sq - GRAVITY_MAGNITUDE_SQ).abs();

    if accel_sq > SHAKE_THRESHOLD_SQ {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let last = LAST_SHAKE_TIME_MS.load(Ordering::Relaxed);
        if now_ms.saturating_sub(last) > SHAKE_COOLDOWN_MS {
            LAST_SHAKE_TIME_MS.store(now_ms, Ordering::Relaxed);
            STATE.shake_count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Called from JNI when step count updates.
pub(crate) fn on_step_count(steps: i32) {
    if !STATE.steps_active.load(Ordering::Relaxed) {
        return;
    }
    STATE.step_count.store(steps, Ordering::Relaxed);
}
