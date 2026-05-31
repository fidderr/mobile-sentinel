//! Torch (flashlight) capability. Gated behind `torch`.
//!
//! The only entry point for the camera flash LED. Not reachable through any
//! raw sink handle without enabling the `torch` feature. The JNI calls into
//! `com.mobilesentinel.SentinelTorchPrimitives` (`:sentinel-torch` module)
//! live here — the shared plumbing is `platform::android::jni`.

use crate::android_or;

/// Turn the torch on. `_brightness` (0.0–1.0) is honoured where supported.
pub fn on(_brightness: Option<f32>) {
    android_or!(
        {
            let _ = android::torch_on();
        },
        ()
    )
}

/// Turn the torch off.
pub fn off() {
    android_or!(
        {
            let _ = android::torch_off();
        },
        ()
    )
}

/// Whether the device has a torch.
pub fn is_available() -> bool {
    android_or!(android::has_torch(), false)
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    const TORCH: &str = "com/mobilesentinel/SentinelTorchPrimitives";

    pub(super) fn torch_on() -> bool {
        with_jni_class(TORCH, false, |env, class| {
            env.call_static_method(class, "turnOn", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn torch_off() -> bool {
        with_jni_class(TORCH, false, |env, class| {
            env.call_static_method(class, "turnOff", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn has_torch() -> bool {
        with_jni_class(TORCH, false, |env, class| {
            env.call_static_method(class, "hasTorch", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}
