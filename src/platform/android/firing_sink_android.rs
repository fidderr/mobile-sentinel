//! `AndroidFiringSink` — the JNI-backed [`FiringSink`] implementation.
//!
//! Drives the "alarm is firing" platform surface via the per-feature Kotlin
//! primitives (each in its own `:sentinel-<feature>` Gradle module), in the
//! precise order the alarm pipeline requires (foreground service → audio →
//! kiosk → full-screen intent → wake lock).
//!
//! All JNI for the firing surface lives HERE (not in a shared facade): each
//! surface is gated by its own firing sub-feature, so a consumer that
//! enables e.g. only `exact-alarm` + `foreground-service` compiles none of
//! the kiosk / full-screen-intent / audio code. Disabled surfaces degrade to
//! no-ops (and `system_default_sound_uri` falls back to a silent URI when
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
    // `req` is consumed by the foreground-service / firing-audio / kiosk /
    // full-screen-intent surfaces. If a build enables none of those (e.g.
    // only `wake-lock`), the parameter is genuinely unused.
    #[cfg_attr(
        not(any(
            feature = "foreground-service",
            feature = "firing-audio",
            feature = "kiosk",
            feature = "full-screen-intent"
        )),
        allow(unused_variables)
    )]
    fn start_firing(&self, req: &FireRequest) -> bool {
        // Foreground service first — keeps the Rust process alive after a
        // swipe-kill while the alarm fires. When `foreground-service` is off
        // there is no critical surface to engage, so report success.
        #[cfg(feature = "foreground-service")]
        let fgs_ok = {
            // Rust decides whether the full-screen intent / banner is needed:
            // it is redundant when the firing activity is ALREADY in the
            // foreground (the UI is visible), so suppress it then. The
            // foreground check is a core (capability-agnostic) query into the
            // activity tracker — the FGS Kotlin no longer decides this.
            let want_full_screen = req.firing_full_screen
                && match req.activity_fqcn.as_deref() {
                    Some(fqcn) => !crate::platform::android::is_activity_resumed(fqcn),
                    None => true,
                };
            let ok = jni::start_foreground_service(
                &req.channel_id,
                &req.channel_name,
                &req.title,
                &req.body,
                req.importance,
                req.bypass_dnd,
                req.activity_fqcn.as_deref(),
                want_full_screen,
            );
            if !ok {
                log::error!(
                    "[AndroidFiringSink] start_foreground_service FAILED for {}",
                    req.instance_id
                );
            }
            ok
        };
        #[cfg(not(feature = "foreground-service"))]
        let fgs_ok = true;

        // Audio. Skip for silent (empty / "silent://") so the platform
        // doesn't play system default.
        #[cfg(feature = "firing-audio")]
        if !req.sound_uri.is_empty() && req.sound_uri != "silent://" {
            let audio_ok = jni::play_sound(
                &req.sound_uri,
                &req.audio_usage,
                &req.audio_content_type,
                req.looping,
            );
            if !audio_ok {
                log::warn!(
                    "[AndroidFiringSink] play_sound failed for {} ({})",
                    req.instance_id,
                    req.sound_uri
                );
            }
        }

        // Vibration. A looping waveform that runs until stop_firing / pause.
        // Independent of audio so vibrate-only alarms work.
        #[cfg(feature = "firing-vibration")]
        if req.vibrate {
            let pattern: &[i64] = if req.vibration_pattern.is_empty() {
                // Default alarm buzz: pause, long buzz, short gap, long buzz.
                &[0, 400, 200, 400]
            } else {
                &req.vibration_pattern
            };
            if !jni::start_vibration(pattern) {
                log::warn!(
                    "[AndroidFiringSink] start_vibration failed for {}",
                    req.instance_id
                );
            }
        }

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
                    req.kiosk_debounce_ms,
                    req.kiosk_hide_status_bar,
                    req.kiosk_hide_nav_bar,
                );
            }
        }

        // Full-screen intent — wakes the lock screen and shows the activity.
        #[cfg(feature = "full-screen-intent")]
        if req.firing_full_screen {
            if let Some(fqcn) = &req.activity_fqcn {
                let _ = jni::show_full_screen_intent(fqcn, true);
            }
        }

        // Wake lock for the firing duration.
        #[cfg(feature = "wake-lock")]
        let _ = jni::acquire_wake_lock("sentinel.firing", 5 * 60 * 1000);

        fgs_ok
    }

    fn stop_firing(&self, instance_id: &str) {
        #[cfg(feature = "firing-audio")]
        let _ = jni::stop_sound();
        #[cfg(feature = "firing-vibration")]
        let _ = jni::stop_vibration();
        #[cfg(feature = "wake-lock")]
        let _ = jni::release_wake_lock("sentinel.firing");
        #[cfg(feature = "kiosk")]
        let _ = jni::disable_kiosk_mode();
        #[cfg(feature = "foreground-service")]
        let _ = jni::stop_foreground_service();
        log::info!(
            "[AndroidFiringSink] stop_firing complete for {}",
            instance_id
        );
    }

    fn pause_audio(&self) {
        // Pause = stop the firing audio (and vibration). Bringing the alarm
        // back is a full re-engage (start_firing) issued by the recipe's
        // Resume handler, so there is no separate resume path on Android.
        #[cfg(feature = "firing-audio")]
        let _ = jni::stop_sound();
        #[cfg(feature = "firing-vibration")]
        let _ = jni::stop_vibration();
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
    const FGS: &str = "com/mobilesentinel/SentinelForegroundServicePrimitives";
    #[cfg(feature = "firing-audio")]
    const FIRING_AUDIO: &str = "com/mobilesentinel/SentinelFiringAudioPrimitives";
    #[cfg(feature = "firing-vibration")]
    const FIRING_VIBRATION: &str = "com/mobilesentinel/SentinelFiringVibrationPrimitives";
    #[cfg(feature = "exact-alarm")]
    const EXACT_ALARM: &str = "com/mobilesentinel/SentinelExactAlarmPrimitives";
    #[cfg(feature = "kiosk")]
    const KIOSK: &str = "com/mobilesentinel/SentinelKioskPrimitives";
    #[cfg(feature = "full-screen-intent")]
    const FULL_SCREEN_INTENT: &str = "com/mobilesentinel/SentinelFullScreenIntentPrimitives";
    #[cfg(feature = "wake-lock")]
    const WAKE_LOCK: &str = "com/mobilesentinel/SentinelWakeLockPrimitives";
    #[cfg(any(feature = "alarm-kit", feature = "firing-audio"))]
    const CORE_PRIMITIVES: &str = "com/mobilesentinel/SentinelPrimitives";

    #[cfg(feature = "foreground-service")]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_foreground_service(
        channel_id: &str,
        channel_name: &str,
        title: &str,
        body: &str,
        importance: i32,
        bypass_dnd: bool,
        activity_fqcn: Option<&str>,
        full_screen: bool,
    ) -> bool {
        with_jni_class(FGS, false, |env, class| {
            let s_channel = jni_str(env, channel_id)?;
            let s_name = jni_str(env, channel_name)?;
            let s_title = jni_str(env, title)?;
            let s_body = jni_str(env, body)?;
            let s_fqcn = match activity_fqcn {
                Some(f) => jni_str(env, f)?.into(),
                None => JObject::null(),
            };
            let res = env.call_static_method(
                class,
                "startForegroundService",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;IZLjava/lang/String;Z)Z",
                &[
                    JValue::Object(&s_channel.into()),
                    JValue::Object(&s_name.into()),
                    JValue::Object(&s_title.into()),
                    JValue::Object(&s_body.into()),
                    JValue::Int(importance as jint),
                    JValue::Bool(bypass_dnd as jboolean),
                    JValue::Object(&s_fqcn),
                    JValue::Bool(full_screen as jboolean),
                ],
            );
            res.ok().and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "foreground-service")]
    pub(super) fn stop_foreground_service() -> bool {
        with_jni_class(FGS, false, |env, class| {
            env.call_static_method(class, "stopForegroundService", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "firing-audio")]
    pub(super) fn play_sound(uri: &str, usage: &str, content_type: &str, looping: bool) -> bool {
        with_jni_class(FIRING_AUDIO, false, |env, class| {
            let s_uri = jni_str(env, uri)?;
            let s_usage = jni_str(env, usage)?;
            let s_ct = jni_str(env, content_type)?;
            env.call_static_method(
                class,
                "playSound",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)Z",
                &[
                    JValue::Object(&s_uri.into()),
                    JValue::Object(&s_usage.into()),
                    JValue::Object(&s_ct.into()),
                    JValue::Bool(looping as jboolean),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "firing-audio")]
    pub(super) fn stop_sound() -> bool {
        with_jni_class(FIRING_AUDIO, false, |env, class| {
            env.call_static_method(class, "stopSound", "()Z", &[])
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

    #[cfg(feature = "firing-vibration")]
    pub(super) fn start_vibration(pattern: &[i64]) -> bool {
        with_jni_class(FIRING_VIBRATION, false, |env, class| {
            let arr = env.new_long_array(pattern.len() as i32).ok()?;
            env.set_long_array_region(&arr, 0, pattern).ok()?;
            env.call_static_method(
                class,
                "startVibration",
                "([J)Z",
                &[JValue::Object(&arr.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "firing-vibration")]
    pub(super) fn stop_vibration() -> bool {
        with_jni_class(FIRING_VIBRATION, false, |env, class| {
            env.call_static_method(class, "stopVibration", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
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
        relaunch_debounce_ms: u32,
        hide_status_bar: bool,
        hide_nav_bar: bool,
    ) -> bool {
        with_jni_class(KIOSK, false, |env, class| {
            let s = jni_str(env, activity_fqcn)?;
            env.call_static_method(
                class,
                "enableKioskMode",
                "(Ljava/lang/String;ZZZIZZ)Z",
                &[
                    JValue::Object(&s.into()),
                    JValue::Bool(block_home as jboolean),
                    JValue::Bool(block_back as jboolean),
                    JValue::Bool(block_recents as jboolean),
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

    #[cfg(feature = "wake-lock")]
    pub(super) fn acquire_wake_lock(tag: &str, timeout_ms: u64) -> bool {
        with_jni_class(WAKE_LOCK, false, |env, class| {
            let s = jni_str(env, tag)?;
            env.call_static_method(
                class,
                "acquireWakeLock",
                "(Ljava/lang/String;J)Z",
                &[JValue::Object(&s.into()), JValue::Long(timeout_ms as jlong)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    #[cfg(feature = "wake-lock")]
    pub(super) fn release_wake_lock(tag: &str) -> bool {
        with_jni_class(WAKE_LOCK, false, |env, class| {
            let s = jni_str(env, tag)?;
            env.call_static_method(
                class,
                "releaseWakeLock",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s.into())],
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
