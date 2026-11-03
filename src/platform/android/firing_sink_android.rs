//! `AndroidFiringSink` — the JNI-backed [`FiringSink`] implementation.
//!
//! Drives the "alarm is firing" platform surface via the per-feature Kotlin
//! primitives, in the precise order the alarm pipeline requires (ring → kiosk
//! → full-screen intent). Sound, vibration and the foreground notification are
//! one command to the ring service in `:sentinel` (`SentinelRing`), which
//! usually rings already by then: the alarm starts it from the plan it was armed
//! with.
//!
//! All JNI for the firing surface lives HERE (not in a shared facade): each
//! surface is gated by its own firing sub-feature, so a consumer that
//! enables e.g. only `exact-alarm` + `foreground-service` compiles none of
//! the kiosk / full-screen-intent code. Disabled surfaces degrade to no-ops
//! (and `system_default_sound_uri` falls back to a silent URI when
//! `firing-audio` is off). The only shared dependency is the JNI plumbing in
//! [`crate::platform::android::jni`].

use crate::firing::{ExactAlarmRequest, FireRequest, FiringSink};

/// JNI-backed firing surface for Android.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidFiringSink;

impl AndroidFiringSink {
    pub fn new() -> Self {
        Self
    }
}

impl FiringSink for AndroidFiringSink {
    // `req` is consumed by the foreground-service / kiosk / full-screen-intent
    // surfaces. If a build enables none of those, the parameter is genuinely
    // unused.
    #[cfg_attr(
        not(any(
            feature = "foreground-service",
            feature = "kiosk",
            feature = "full-screen-intent"
        )),
        allow(unused_variables)
    )]
    fn start_firing(&self, req: &FireRequest) -> bool {
        // Whether the UI is where it has to be already. For a ring that comes back that is
        // anywhere in this app: closing what the user is in would be no help.
        let ui_in_front = || {
            if req.fresh {
                req.activity_fqcn
                    .as_deref()
                    .is_some_and(crate::platform::android::is_activity_resumed)
            } else {
                crate::platform::android::is_app_in_front()
            }
        };

        // The ring first. When `foreground-service` is off there is no critical
        // surface to engage, so report success.
        #[cfg(feature = "foreground-service")]
        let ring_ok = {
            // The service leaves out the full-screen banner when the firing UI is
            // already on screen: it would only cover it.
            let ui_visible = ui_in_front();
            let ok = serde_json::to_string(req)
                .is_ok_and(|plan| jni::start_ring(&req.instance_id, &plan, ui_visible));
            if !ok {
                log::error!(
                    "[AndroidFiringSink] start_ring FAILED for {}",
                    req.instance_id
                );
            }
            ok
        };
        #[cfg(not(feature = "foreground-service"))]
        let ring_ok = true;

        // Kiosk relaunch policy — MUST engage BEFORE the FSI launch so the
        // activity sees the persisted kiosk state before any HOME/BACK can
        // escape. The block toggles + immersive flags are policy passed from
        // the recipe layer (the `FireRequest`), not hardcoded in Kotlin.
        #[cfg(feature = "kiosk")]
        if req.kiosk_mode {
            if let Some(fqcn) = &req.activity_fqcn {
                let _ = jni::enable_kiosk_mode(
                    fqcn,
                    req.kiosk_block_home,
                    req.kiosk_block_back,
                    req.kiosk_block_recents,
                    req.kiosk_block_volume,
                    req.kiosk_debounce_ms,
                    req.kiosk_hide_status_bar,
                    req.kiosk_hide_nav_bar,
                );
            }
        }

        // Full-screen intent — wakes the lock screen and shows the activity.
        // Skipped when the activity is already in front: launching it again
        // only delivers a new intent, which pauses and resumes it.
        #[cfg(feature = "full-screen-intent")]
        if req.firing_full_screen {
            if let Some(fqcn) = &req.activity_fqcn {
                if !ui_in_front() {
                    let _ = jni::show_full_screen_intent(fqcn, true);
                }
            }
        }

        ring_ok
    }

    fn stop_firing(&self, instance_id: &str) {
        #[cfg(feature = "foreground-service")]
        let _ = jni::stop_ring(instance_id);
        log::info!(
            "[AndroidFiringSink] stop_firing complete for {}",
            instance_id
        );
    }

    fn release_takeover(&self) {
        #[cfg(feature = "kiosk")]
        let _ = jni::disable_kiosk_mode();
    }

    fn pause_audio(&self) {
        // Bringing the alarm back is a full re-engage (start_firing) issued by
        // the recipe's Resume handler, so there is no separate resume path.
        #[cfg(feature = "foreground-service")]
        let _ = jni::pause_ring();
    }

    fn schedule_exact_alarm(&self, req: &ExactAlarmRequest) -> bool {
        #[cfg(feature = "exact-alarm")]
        {
            jni::schedule_exact_alarm(
                &req.alarm_id,
                req.target_unix_ms,
                req.metadata_json.as_deref(),
            )
        }
        #[cfg(not(feature = "exact-alarm"))]
        {
            let _ = req;
            false
        }
    }

    fn cancel_exact_alarm(&self, alarm_id: &str) {
        #[cfg(feature = "exact-alarm")]
        let _ = jni::cancel_exact_alarm(alarm_id);
        #[cfg(not(feature = "exact-alarm"))]
        let _ = alarm_id;
    }

    fn system_default_sound_uri(&self) -> String {
        #[cfg(feature = "firing-audio")]
        {
            jni::get_system_default_sound_uri()
        }
        #[cfg(not(feature = "firing-audio"))]
        {
            "silent://".to_owned()
        }
    }
}

/// IANA time-zone id from core `SentinelPrimitives.getTimeZoneId()`. Used by
/// the recipe layer (AlarmKit) to resolve schedules in the user's local zone.
/// A generic device query in core, so it needs no firing module.
#[cfg(feature = "alarm-kit")]
pub(crate) fn get_time_zone_id() -> String {
    jni::get_time_zone_id()
}

// ---------------------------------------------------------------------------
// JNI calls into the per-feature Kotlin primitives. Each is gated by the
// firing sub-feature that uses it, so a disabled surface compiles none of its
// marshalling — and targets only that feature's own Gradle module's class.
// ---------------------------------------------------------------------------
mod jni {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    #[allow(unused_imports)]
    use ::jni::objects::{JObject, JString, JValue};
    #[allow(unused_imports)]
    use ::jni::sys::{jboolean, jint, jlong};

    #[cfg(feature = "foreground-service")]
    const RING: &str = "com/mobilesentinel/SentinelRing";
    #[cfg(feature = "exact-alarm")]
    const EXACT_ALARM: &str = "com/mobilesentinel/SentinelExactAlarmPrimitives";
    #[cfg(feature = "kiosk")]
    const KIOSK: &str = "com/mobilesentinel/SentinelKioskPrimitives";
    #[cfg(feature = "full-screen-intent")]
    const FULL_SCREEN_INTENT: &str = "com/mobilesentinel/SentinelFullScreenIntentPrimitives";
    #[cfg(any(feature = "alarm-kit", feature = "firing-audio"))]
    const CORE_PRIMITIVES: &str = "com/mobilesentinel/SentinelPrimitives";

    #[cfg(feature = "foreground-service")]
    pub(super) fn start_ring(id: &str, plan_json: &str, ui_visible: bool) -> bool {
        with_jni_class(RING, false, |env, class| {
            let s_id = jni_str(env, id)?;
            let s_plan = jni_str(env, plan_json)?;
            env.call_static_method(
                class,
                "startRing",
                "(Ljava/lang/String;Ljava/lang/String;Z)Z",
                &[
                    JValue::Object(&s_id.into()),
                    JValue::Object(&s_plan.into()),
                    JValue::Bool(ui_visible as jboolean),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "foreground-service")]
    pub(super) fn stop_ring(id: &str) -> bool {
        with_jni_class(RING, false, |env, class| {
            let s_id = jni_str(env, id)?;
            env.call_static_method(
                class,
                "stopRing",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s_id.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "foreground-service")]
    pub(super) fn pause_ring() -> bool {
        with_jni_class(RING, false, |env, class| {
            env.call_static_method(class, "pauseRing", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "firing-audio")]
    pub(super) fn get_system_default_sound_uri() -> String {
        with_jni_class(CORE_PRIMITIVES, String::new(), |env, class| {
            let res = env
                .call_static_method(
                    class,
                    "getSystemDefaultSoundUri",
                    "()Ljava/lang/String;",
                    &[],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }

    #[cfg(feature = "exact-alarm")]
    pub(super) fn schedule_exact_alarm(
        id: &str,
        target_time_ms: i64,
        metadata_json: Option<&str>,
    ) -> bool {
        with_jni_class(EXACT_ALARM, false, |env, class| {
            let s_id = jni_str(env, id)?;
            let s_meta = match metadata_json {
                Some(m) => jni_str(env, m)?.into(),
                None => JObject::null(),
            };
            env.call_static_method(
                class,
                "scheduleExactAlarm",
                "(Ljava/lang/String;JLjava/lang/String;)Z",
                &[
                    JValue::Object(&s_id.into()),
                    JValue::Long(target_time_ms as jlong),
                    JValue::Object(&s_meta),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "exact-alarm")]
    pub(super) fn cancel_exact_alarm(id: &str) -> bool {
        with_jni_class(EXACT_ALARM, false, |env, class| {
            let s = jni_str(env, id)?;
            env.call_static_method(
                class,
                "cancelExactAlarm",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "kiosk")]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn enable_kiosk_mode(
        activity_fqcn: &str,
        block_home: bool,
        block_back: bool,
        block_recents: bool,
        block_volume: bool,
        relaunch_debounce_ms: u32,
        hide_status_bar: bool,
        hide_nav_bar: bool,
    ) -> bool {
        with_jni_class(KIOSK, false, |env, class| {
            let s = jni_str(env, activity_fqcn)?;
            env.call_static_method(
                class,
                "enableKioskMode",
                "(Ljava/lang/String;ZZZZIZZ)Z",
                &[
                    JValue::Object(&s.into()),
                    JValue::Bool(block_home as jboolean),
                    JValue::Bool(block_back as jboolean),
                    JValue::Bool(block_recents as jboolean),
                    JValue::Bool(block_volume as jboolean),
                    JValue::Int(relaunch_debounce_ms as jint),
                    JValue::Bool(hide_status_bar as jboolean),
                    JValue::Bool(hide_nav_bar as jboolean),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "kiosk")]
    pub(super) fn disable_kiosk_mode() -> bool {
        with_jni_class(KIOSK, false, |env, class| {
            env.call_static_method(class, "disableKioskMode", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "full-screen-intent")]
    pub(super) fn show_full_screen_intent(activity_fqcn: &str, dismiss_keyguard: bool) -> bool {
        with_jni_class(FULL_SCREEN_INTENT, false, |env, class| {
            let s = jni_str(env, activity_fqcn)?;
            env.call_static_method(
                class,
                "showFullScreenIntent",
                "(Ljava/lang/String;Z)Z",
                &[
                    JValue::Object(&s.into()),
                    JValue::Bool(dismiss_keyguard as jboolean),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "alarm-kit")]
    pub(super) fn get_time_zone_id() -> String {
        with_jni_class(CORE_PRIMITIVES, String::new(), |env, class| {
            let res = env
                .call_static_method(class, "getTimeZoneId", "()Ljava/lang/String;", &[])
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }
}
