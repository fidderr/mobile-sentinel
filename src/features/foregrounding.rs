//! Foregrounding capability — finish the app's activity. Gated behind
//! `foregrounding`. Sole entry point; not a trait method. The JNI call into
//! `com.mobilesentinel.SentinelForegroundingPrimitives` lives here.

use crate::android_or;

/// Finish the current activity and remove its task (fully close the app
/// after the consumer's work is done, e.g. snooze / dismiss). The caller
/// should have disabled kiosk mode first so the relaunch watchdog doesn't
/// bring the activity straight back.
pub fn finish_activity() {
    android_or!(
        {
            let _ = android::finish_activity();
        },
        ()
    )
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    const FOREGROUNDING: &str = "com/mobilesentinel/SentinelForegroundingPrimitives";

    pub(super) fn finish_activity() -> bool {
        with_jni_class(FOREGROUNDING, false, |env, class| {
            env.call_static_method(class, "finishActivity", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}
