//! Audio preview capability — play arbitrary audio for UI preview.
//!
//! Gated behind the `audio` Cargo feature. This is **non-firing** audio
//! (e.g. previewing an alarm tone in a settings screen). The firing
//! pipeline's looping alarm audio is the firing surface (`FiringSink`) and
//! is not exposed here.
//!
//! The only entry point is this module, so the feature gate cannot be
//! bypassed. The JNI calls into
//! `com.mobilesentinel.SentinelAudioPreviewPrimitives` live here.
//!
//! ```toml
//! mobile-sentinel = { version = "...", features = ["audio"] }
//! ```

use crate::error::SentinelError;
use crate::types::PlaybackHandle;

/// Play audio from a file path / URI, returning a handle for later stop.
pub fn play(uri: &str, looping: bool) -> Result<PlaybackHandle, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let id = android::audio_play(uri, looping);
        if id > 0 {
            Ok(PlaybackHandle::from_id(id as u64))
        } else {
            Err(SentinelError::unavailable("audio::play"))
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (uri, looping);
        Err(SentinelError::unavailable("audio::play"))
    }
}

/// Stop a handle returned by [`play`].
pub fn stop(handle: &PlaybackHandle) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::audio_stop(handle.id() as i64) {
            Ok(())
        } else {
            Err(SentinelError::unavailable("audio::stop"))
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = handle;
        Err(SentinelError::unavailable("audio::stop"))
    }
}

/// Set the global media playback volume (0.0–1.0).
pub fn set_volume(volume: f32) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::audio_set_volume(volume) {
            Ok(())
        } else {
            Err(SentinelError::unavailable("audio::set_volume"))
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = volume;
        Err(SentinelError::unavailable("audio::set_volume"))
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::JValue;
    use jni::sys::jlong;

    const AUDIO_PREVIEW: &str = "com/mobilesentinel/SentinelAudioPreviewPrimitives";

    /// Returns a playback handle id (>0) or 0 on failure.
    pub(super) fn audio_play(uri: &str, looping: bool) -> i64 {
        with_jni_class(AUDIO_PREVIEW, 0i64, |env, class| {
            let s = jni_str(env, uri)?;
            env.call_static_method(
                class,
                "play",
                "(Ljava/lang/String;Z)J",
                &[JValue::Object(&s.into()), JValue::Bool(looping as u8)],
            )
            .ok()
            .and_then(|v| v.j().ok())
        })
    }

    pub(super) fn audio_stop(handle: i64) -> bool {
        with_jni_class(AUDIO_PREVIEW, false, |env, class| {
            env.call_static_method(class, "stop", "(J)Z", &[JValue::Long(handle as jlong)])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn audio_set_volume(volume: f32) -> bool {
        with_jni_class(AUDIO_PREVIEW, false, |env, class| {
            env.call_static_method(class, "setVolume", "(F)Z", &[JValue::Float(volume)])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}
