//! Foregrounding capability — finish the app's activity, move it to the
//! background, and handle Back. Gated behind `foregrounding`. Sole entry point;
//! not a trait method. The JNI call into
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

/// Have `callback` called for Back instead of the system finishing the activity,
/// which ends the process in a Dioxus app. Works the same on every Android version.
/// It runs on Android's main thread, so it should only pass the news on. While a
/// kiosk blocks Back it does not get called on Android 13 and up, and below that
/// nothing may have changed, so a screen that must not be left ignores it.
pub fn on_back<F>(callback: F)
where
    F: Fn() + Send + Sync + 'static,
{
    android_or!(
        {
            *android::ON_BACK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(callback));
            let _ = android::route_back();
        },
        {
            let _ = callback;
        }
    )
}

/// Send the app to the background, as Home does, keeping it running.
pub fn move_to_back() {
    android_or!(
        {
            let _ = android::move_to_back();
        },
        ()
    )
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;
    use std::sync::Mutex;

    const FOREGROUNDING: &str = "com/mobilesentinel/SentinelForegroundingPrimitives";

    type BackCallback = Box<dyn Fn() + Send + Sync>;
    pub(super) static ON_BACK: Mutex<Option<BackCallback>> = Mutex::new(None);

    #[no_mangle]
    pub extern "system" fn Java_com_mobilesentinel_SentinelForegroundingPrimitives_nativeOnBack(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
    ) {
        if let Ok(callback) = ON_BACK.lock() {
            if let Some(callback) = callback.as_ref() {
                callback();
            }
        }
    }

    pub(super) fn route_back() -> bool {
        with_jni_class(FOREGROUNDING, false, |env, class| {
            env.call_static_method(class, "routeBack", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn move_to_back() -> bool {
        with_jni_class(FOREGROUNDING, false, |env, class| {
            env.call_static_method(class, "moveToBack", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn finish_activity() -> bool {
        with_jni_class(FOREGROUNDING, false, |env, class| {
            env.call_static_method(class, "finishActivity", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}
