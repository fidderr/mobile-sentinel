//! Global sensor state for accelerometer and step counter.
//! Kotlin delivers sensor events via JNI → these are stored here.
//! Consumer UI polls the values via the public API.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::features::sensors::recorder::Recorder;
use crate::features::sensors::squat::SquatDetector;

/// Global sensor state — written by JNI callbacks, read by consumer UI.
pub struct SensorState {
    /// Current shake count (detected from accelerometer magnitude spikes).
    pub shake_count: AtomicI32,
    /// Current step count (relative to when step counter was started).
    pub step_count: AtomicI32,
    /// Squats counted by the squat detector since it was started or reset.
    pub squat_count: AtomicI32,
    /// Whether the squat detector gets the accelerometer samples.
    pub squat_active: AtomicBool,
    /// Whether accelerometer is currently active.
    pub accel_active: AtomicBool,
    /// Whether step counter is currently active.
    pub steps_active: AtomicBool,
}

static STATE: Lazy<SensorState> = Lazy::new(|| SensorState {
    shake_count: AtomicI32::new(0),
    step_count: AtomicI32::new(0),
    squat_count: AtomicI32::new(0),
    squat_active: AtomicBool::new(false),
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

/// Reset the squat count and forget the signal so far.
pub fn reset_squat_count() {
    STATE.squat_count.store(0, Ordering::Relaxed);
    if let Ok(mut detector) = SQUAT_DETECTOR.lock() {
        detector.reset();
    }
}

/// Get current squat count.
pub fn get_squat_count() -> i32 {
    STATE.squat_count.load(Ordering::Relaxed)
}

/// Start the accelerometer and count squats from it.
pub fn start_squat_counter() {
    reset_squat_count();
    STATE.squat_active.store(true, Ordering::Relaxed);
    start_accelerometer();
}

/// Stop counting squats and stop the accelerometer.
pub fn stop_squat_counter() {
    STATE.squat_active.store(false, Ordering::Relaxed);
    stop_accelerometer();
}

/// Write every accelerometer sample to `path` (CSV) until [`stop_recording`]. The accelerometer
/// has to be running (`start_accelerometer` or `start_squat_counter`). Returns false if the file
/// cannot be created.
pub fn start_recording(path: &str) -> bool {
    match Recorder::create(std::path::Path::new(path)) {
        Ok(recorder) => {
            if let Ok(mut slot) = RECORDER.lock() {
                *slot = Some(recorder);
                return true;
            }
            false
        }
        Err(e) => {
            log::error!("[sensor_state] cannot record to {path}: {e}");
            false
        }
    }
}

/// Stop recording and close the file. Returns how many samples were written.
pub fn stop_recording() -> u64 {
    match RECORDER.lock() {
        Ok(mut slot) => slot.take().map_or(0, |r| r.samples()),
        Err(_) => 0,
    }
}

/// Get current step count.
pub fn get_step_count() -> i32 {
    STATE.step_count.load(Ordering::Relaxed)
}

/// Call a method of the Kotlin helper that takes nothing.
#[cfg(target_os = "android")]
fn call_helper(method: &str, signature: &str) {
    let called = crate::platform::android::jni::with_jni_class(
        "com/mobilesentinel/SentinelSensorHelper",
        false,
        |env, class| {
            let result = env.call_static_method(class, method, signature, &[]);
            if result.is_err() {
                let _ = env.exception_clear();
            }
            Some(result.is_ok())
        },
    );
    if called {
        log::info!("[sensor_state] {method}");
    } else {
        log::error!("[sensor_state] {method} failed");
    }
}

/// Start the accelerometer for shake detection.
#[cfg(target_os = "android")]
pub fn start_accelerometer() {
    STATE.shake_count.store(0, Ordering::Relaxed);
    STATE.accel_active.store(true, Ordering::Relaxed);
    call_helper("startAccelerometer", "()Z");
}

/// Stop the accelerometer.
#[cfg(target_os = "android")]
pub fn stop_accelerometer() {
    STATE.accel_active.store(false, Ordering::Relaxed);
    call_helper("stopAccelerometer", "()V");
}

/// Start the step counter.
/// Permission should already be granted (requested when adding the challenge).
#[cfg(target_os = "android")]
pub fn start_step_counter() {
    STATE.step_count.store(0, Ordering::Relaxed);
    STATE.steps_active.store(true, Ordering::Relaxed);
    call_helper("startStepCounter", "()Z");
}

/// Stop the step counter.
#[cfg(target_os = "android")]
pub fn stop_step_counter() {
    STATE.steps_active.store(false, Ordering::Relaxed);
    call_helper("stopStepCounter", "()V");
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

static SQUAT_DETECTOR: Lazy<Mutex<SquatDetector>> =
    Lazy::new(|| Mutex::new(SquatDetector::default()));

static RECORDER: Lazy<Mutex<Option<Recorder>>> = Lazy::new(|| Mutex::new(None));

/// Called from JNI when accelerometer data arrives, with the sensor's own timestamp
/// (nanoseconds).
/// Records the sample if a recording is open, feeds the squat detector if it is started, and
/// detects shakes by checking if the motion-only magnitude exceeds a threshold, gated by a
/// cooldown so a single gesture counts once.
pub(crate) fn on_accelerometer_data(x: f64, y: f64, z: f64, timestamp_ns: i64) {
    if !STATE.accel_active.load(Ordering::Relaxed) {
        return;
    }

    if let Ok(mut slot) = RECORDER.lock() {
        if let Some(recorder) = slot.as_mut() {
            if recorder.write(timestamp_ns, x, y, z).is_err() {
                log::error!("[sensor_state] recording failed, stopped");
                *slot = None;
            }
        }
    }

    if STATE.squat_active.load(Ordering::Relaxed) {
        if let Ok(mut detector) = SQUAT_DETECTOR.lock() {
            detector.push(timestamp_ns as f64 * 1e-9, x, y, z);
            STATE
                .squat_count
                .store(detector.count() as i32, Ordering::Relaxed);
        }
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
