//! Android Activity/Context access via JNI.
//! Provides safe wrappers for obtaining the Android Activity context
//! using ndk-context, which receives the Activity pointer from the host
//! Rust mobile framework (Dioxus, Tauri, etc.).

#[cfg(target_os = "android")]
use jni::objects::{GlobalRef, JObject};

use crate::error::SentinelError;

/// Get the current Android Activity as a JNI GlobalRef.
/// Uses ndk-context to get the Activity pointer provided by the host framework.
/// That panics until the framework has an Activity, so this is only for what the
/// user started from the UI; anything else goes through `SentinelPrimitives`.
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
