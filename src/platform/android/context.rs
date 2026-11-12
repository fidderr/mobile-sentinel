//! Android Activity/Context access via JNI.
//! Provides safe wrappers for obtaining the Android Activity context
//! through the Activity tracker, with ndk-context (which receives a pointer from the host
//! Rust mobile framework: Dioxus, Tauri, a native host) as the fallback.

#[cfg(target_os = "android")]
use jni::objects::{GlobalRef, JObject};

use crate::error::SentinelError;

/// The Activity that is resumed now, from the Kotlin `SentinelActivityTracker`.
#[cfg(target_os = "android")]
fn resumed_activity() -> Option<GlobalRef> {
    use crate::platform::android::jni::with_jni_class;

    with_jni_class(
        "com/mobilesentinel/SentinelActivityTracker",
        None,
        |env, class| {
            let activity = env
                .call_static_method(
                    class,
                    "getCurrentResumedActivity",
                    "()Landroid/app/Activity;",
                    &[],
                )
                .ok()?
                .l()
                .ok()?;
            if activity.is_null() {
                return None;
            }
            Some(env.new_global_ref(activity).ok())
        },
    )
}

/// Get the current Android Activity as a JNI GlobalRef.
/// That is the resumed Activity the Kotlin `SentinelActivityTracker` knows, because what
/// ndk-context holds depends on the host framework: a native host built on
/// android-activity puts the Application there, which is no Activity. Only without a
/// resumed Activity does it use the pointer ndk-context holds. That panics until the
/// framework has set it, so this is only for what the user started from the UI; anything
/// else goes through `SentinelPrimitives`.
#[cfg(target_os = "android")]
#[allow(dead_code)] // Which capabilities need it depends on the enabled features.
pub fn get_activity() -> Result<(jni::JavaVM, GlobalRef), SentinelError> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| {
        SentinelError::RuntimeError {
            code: crate::error::ErrorCode::Internal,
            message: format!("Failed to get JavaVM: {}", e),
        }
    })?;

    if let Some(tracked) = resumed_activity() {
        return Ok((vm, tracked));
    }

    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };

    // Scope the env borrow so we can move `vm` into the return value.
    let global_ref = {
        let env = vm
            .attach_current_thread()
            .map_err(|e| SentinelError::RuntimeError {
                code: crate::error::ErrorCode::Internal,
                message: format!("Failed to attach thread: {}", e),
            })?;

        env.new_global_ref(activity)
            .map_err(|e| SentinelError::RuntimeError {
                code: crate::error::ErrorCode::Internal,
                message: format!("Failed to create global ref: {}", e),
            })?
    };

    Ok((vm, global_ref))
}
