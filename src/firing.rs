//! `FiringSink` — the small, injectable interface the recipe engine uses to
//! drive the "alarm is firing" platform surface.
//!
//! This is deliberately NOT the old god-trait. It is the minimal set of
//! operations the recipe state machine (AlarmClass / AlarmKit) needs:
//! engage/tear-down the firing surface, pause/resume audio, schedule/cancel
//! the OS exact alarm, and resolve the system-default sound. Everything a
//! *consumer* calls directly lives in the per-capability `features/` modules
//! instead.
//!
//! Two implementations: `AndroidFiringSink` (JNI, in `platform::android`)
//! and [`MockFiringSink`] (records calls, for recipe tests).

use std::sync::Arc;

use once_cell::sync::OnceCell;

/// Everything the recipe engine needs to fire an alarm. Built by the recipe
/// from a freshly-loaded Context.
#[derive(Debug, Clone, PartialEq)]
pub struct FireRequest {
    pub instance_id: String,
    /// Notification channel id for the firing FGS notification.
    pub channel_id: String,
    /// Notification channel display name (used on first-create only).
    pub channel_name: String,
    /// Notification title shown while firing.
    pub title: String,
    /// Notification body shown while firing.
    pub body: String,
    /// Importance level (Android `NotificationManager.IMPORTANCE_*`).
    pub importance: i32,
    /// Whether the channel should bypass DND.
    pub bypass_dnd: bool,
    /// Resolved playable sound URI. Empty string / `"silent://"` = silent.
    pub sound_uri: String,
    /// AudioAttributes usage tag (e.g. `"alarm"`).
    pub audio_usage: String,
    /// AudioAttributes content type (e.g. `"sonification"`).
    pub audio_content_type: String,
    /// Whether the sound should loop.
    pub looping: bool,
    /// Whether to vibrate while firing (looping waveform). Independent of
    /// `sound_uri`: a consumer can have sound-only, vibrate-only (silent uri +
    /// `vibrate`), or both.
    pub vibrate: bool,
    /// Vibration waveform (alternating wait/vibrate ms) looped while firing.
    /// Empty falls back to the sink's default alarm pattern.
    pub vibration_pattern: Vec<i64>,
    /// Whether kiosk mode (lock-task) should be engaged.
    pub kiosk_mode: bool,
    /// Kiosk: relaunch the activity when the user presses HOME.
    pub kiosk_block_home: bool,
    /// Kiosk: consume the BACK gesture.
    pub kiosk_block_back: bool,
    /// Kiosk: relaunch the activity when the user opens Recents / swipes away.
    pub kiosk_block_recents: bool,
    /// Activity FQCN used for full-screen-intent target + kiosk relaunch.
    pub activity_fqcn: Option<String>,
    /// Debounce for kiosk relaunch.
    pub kiosk_debounce_ms: u32,
    /// While kiosk-firing, hide the status bar.
    pub kiosk_hide_status_bar: bool,
    /// While kiosk-firing, hide the navigation bar.
    pub kiosk_hide_nav_bar: bool,
    /// Use a full-screen-intent notification.
    pub firing_full_screen: bool,
}

/// Configuration for scheduling an OS exact alarm.
#[derive(Debug, Clone, PartialEq)]
pub struct ExactAlarmRequest {
    pub alarm_id: String,
    pub target_unix_ms: i64,
    /// Opaque metadata handed back to Rust on dispatch.
    pub metadata_json: Option<String>,
}

/// The minimal, mockable firing interface used by the recipe engine.
///
/// Implementors compose whatever firing sub-capabilities they support
/// (foreground service, audio, kiosk, full-screen intent, wake lock, exact
/// alarm). The engine only needs these six operations.
pub trait FiringSink: Send + Sync {
    /// Engage the whole firing surface. Returns `true` if the critical
    /// surface (foreground service) engaged.
    fn start_firing(&self, req: &FireRequest) -> bool;
    /// Tear down everything `start_firing` engaged. Idempotent.
    fn stop_firing(&self, instance_id: &str);
    /// Pause firing audio without tearing down other surfaces (e.g. while a
    /// challenge screen is shown). Bring the alarm back with
    /// [`Self::start_firing`] — there is no separate "resume audio" path.
    fn pause_audio(&self);
    /// Schedule an OS exact alarm. Returns `true` on success.
    fn schedule_exact_alarm(&self, req: &ExactAlarmRequest) -> bool;
    /// Cancel a previously scheduled exact alarm. Idempotent.
    fn cancel_exact_alarm(&self, alarm_id: &str);
    /// The platform's system-default alarm sound URI (fallback when a
    /// Recipe has no resolved sound).
    fn system_default_sound_uri(&self) -> String;
}

// ---------------------------------------------------------------------------
// Process-wide installed FiringSink
// ---------------------------------------------------------------------------

static SINK: OnceCell<Arc<dyn FiringSink>> = OnceCell::new();

/// Install the process-wide [`FiringSink`]. Called once on app start (on
/// Android with `AndroidFiringSink`).
pub fn install_firing_sink(sink: Arc<dyn FiringSink>) {
    let _ = SINK.set(sink);
}

/// Borrow the installed [`FiringSink`], if any. `None` on host builds that
/// have not installed one.
pub fn firing_sink() -> Option<Arc<dyn FiringSink>> {
    SINK.get().cloned()
}

// ---------------------------------------------------------------------------
// MockFiringSink — records calls for recipe tests (fires / stops / schedules /
// cancels / pauses), so engine tests can assert what a Recipe did.
// ---------------------------------------------------------------------------

/// In-memory recorder implementing [`FiringSink`] for tests.
#[derive(Debug, Default)]
pub struct MockFiringSink {
    pub fires: std::sync::Mutex<Vec<FireRequest>>,
    pub stops: std::sync::Mutex<Vec<String>>,
    pub schedules: std::sync::Mutex<Vec<ExactAlarmRequest>>,
    pub cancels: std::sync::Mutex<Vec<String>>,
    pub pauses: std::sync::atomic::AtomicU32,
}

impl MockFiringSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fire_count(&self) -> usize {
        self.fires.lock().unwrap().len()
    }
    pub fn last_fire(&self) -> Option<FireRequest> {
        self.fires.lock().unwrap().last().cloned()
    }
    pub fn last_schedule(&self) -> Option<ExactAlarmRequest> {
        self.schedules.lock().unwrap().last().cloned()
    }
    pub fn cancel_count(&self) -> usize {
        self.cancels.lock().unwrap().len()
    }
    pub fn schedule_count(&self) -> usize {
        self.schedules.lock().unwrap().len()
    }
}

impl FiringSink for MockFiringSink {
    fn start_firing(&self, req: &FireRequest) -> bool {
        self.fires.lock().unwrap().push(req.clone());
        true
    }
    fn stop_firing(&self, instance_id: &str) {
        self.stops.lock().unwrap().push(instance_id.to_owned());
    }
    fn pause_audio(&self) {
        self.pauses
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    fn schedule_exact_alarm(&self, req: &ExactAlarmRequest) -> bool {
        self.schedules.lock().unwrap().push(req.clone());
        true
    }
    fn cancel_exact_alarm(&self, alarm_id: &str) {
        self.cancels.lock().unwrap().push(alarm_id.to_owned());
    }
    fn system_default_sound_uri(&self) -> String {
        "content://settings/system/default-alarm".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fire() -> FireRequest {
        FireRequest {
            instance_id: "a".into(),
            channel_id: "c".into(),
            channel_name: "C".into(),
            title: "t".into(),
            body: "b".into(),
            importance: 5,
            bypass_dnd: true,
            sound_uri: "file:///a.mp3".into(),
            audio_usage: "alarm".into(),
            audio_content_type: "sonification".into(),
            looping: true,
            vibrate: true,
            vibration_pattern: vec![0, 400, 200, 400],
            kiosk_mode: false,
            kiosk_block_home: true,
            kiosk_block_back: true,
            kiosk_block_recents: true,
            activity_fqcn: None,
            kiosk_debounce_ms: 50,
            kiosk_hide_status_bar: true,
            kiosk_hide_nav_bar: true,
            firing_full_screen: true,
        }
    }

    #[test]
    fn mock_records_firing_calls() {
        let sink = MockFiringSink::new();
        assert!(sink.start_firing(&sample_fire()));
        assert_eq!(sink.fire_count(), 1);
        sink.stop_firing("a");
        sink.pause_audio();
        assert_eq!(sink.pauses.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn mock_records_exact_alarm_calls() {
        let sink = MockFiringSink::new();
        let req = ExactAlarmRequest {
            alarm_id: "x".into(),
            target_unix_ms: 1,
            metadata_json: None,
        };
        assert!(sink.schedule_exact_alarm(&req));
        sink.cancel_exact_alarm("x");
        assert_eq!(sink.schedule_count(), 1);
        assert_eq!(sink.cancel_count(), 1);
    }

    #[test]
    fn default_system_default_sound_uri_is_well_formed() {
        let sink = MockFiringSink::new();
        assert!(sink.system_default_sound_uri().contains("default-alarm"));
    }
}
