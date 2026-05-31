//! Android `SoundBackend` — the JNI-backed sound-resolution backend used by
//! the Sound Library (`sound-library` feature).
//!
//! The firing surface lives in [`super::firing_sink_android`]
//! (`AndroidFiringSink`); every leaf capability lives in its own
//! `crate::features::*` module. This file only holds the sound backend and
//! its single JNI call (`getSystemDefaultSoundUri` on the core
//! `SentinelPrimitives` Kotlin object — a generic device query, so the
//! sound-library feature needs no firing module), using the shared plumbing
//! in [`crate::platform::android::jni`].

#[cfg(feature = "sound-library")]
use crate::platform::android::jni::with_jni_class;

/// Android `SoundBackend` — resolves the system default alarm URI via JNI.
#[cfg(feature = "sound-library")]
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidSoundBackend;

#[cfg(feature = "sound-library")]
impl crate::features::sound::SoundBackend for AndroidSoundBackend {
    fn system_default_uri(&self) -> String {
        with_jni_class(
            "com/mobilesentinel/SentinelPrimitives",
            String::new(),
            |env, class| {
                let res = env
                    .call_static_method(
                        class,
                        "getSystemDefaultSoundUri",
                        "()Ljava/lang/String;",
                        &[],
                    )
                    .ok()?;
                let obj = res.l().ok()?;
                let jstr: ::jni::objects::JString = obj.into();
                let s: String = env.get_string(&jstr).ok()?.into();
                Some(s)
            },
        )
    }
}
