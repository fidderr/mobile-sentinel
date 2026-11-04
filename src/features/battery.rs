//! Battery-optimization capability. Gated behind `battery`.
//!
//! Query / request exemption from battery optimization (improves alarm
//! reliability on aggressive OEMs). Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelBatteryPrimitives` live here.

use crate::android_or;

/// What the battery is up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerState {
    pub charging: bool,
    /// The system's battery saver is on.
    pub power_save: bool,
    /// 15% or less.
    pub low: bool,
}

/// What the battery is up to now. A device without one counts as on the charger.
pub fn power_state() -> PowerState {
    android_or!(
        android::power_state(),
        PowerState {
            charging: true,
            power_save: false,
            low: false,
        }
    )
}

/// `callback` hears the new state when the charger, the battery saver or a low battery changes
/// anything. It runs on Android's main thread: pass the news on. No-op on host.
pub fn on_power_change<F>(callback: F)
where
    F: Fn(PowerState) + Send + Sync + 'static,
{
    android_or!(
        {
            *android::ON_POWER
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(callback));
            let _ = android::watch_power();
        },
        {
            let _ = callback;
        }
    )
}

/// Whether the app is exempt from battery optimization.
pub fn is_exempt() -> bool {
    android_or!(android::battery_is_exempt(), false)
}

/// Request battery-optimization exemption (shows the system prompt).
pub fn request_exemption() {
    android_or!(
        {
            let _ = android::request_battery_exemption();
        },
        ()
    )
}

/// Open the battery-optimization settings page.
pub fn open_settings() {
    android_or!(
        {
            let _ = android::open_battery_settings();
        },
        ()
    )
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    use super::PowerState;
    use std::sync::Mutex;

    const BATTERY: &str = "com/mobilesentinel/SentinelBatteryPrimitives";

    type PowerCallback = Box<dyn Fn(PowerState) + Send + Sync>;
    pub(super) static ON_POWER: Mutex<Option<PowerCallback>> = Mutex::new(None);

    #[no_mangle]
    pub extern "system" fn Java_com_mobilesentinel_SentinelBatteryPrimitives_nativeOnPowerChange(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
    ) {
        if let Ok(callback) = ON_POWER.lock() {
            if let Some(callback) = callback.as_ref() {
                callback(power_state());
            }
        }
    }

    pub(super) fn power_state() -> PowerState {
        let text = with_jni_class(BATTERY, String::new(), |env, class| {
            let value = env
                .call_static_method(class, "powerState", "()Ljava/lang/String;", &[])
                .ok()?
                .l()
                .ok()?;
            let text: String = env.get_string(&value.into()).ok()?.into();
            Some(text)
        });
        parse_power_state(&text)
    }

    /// "charging,powerSave,low". What cannot be read counts as on the charger.
    fn parse_power_state(text: &str) -> PowerState {
        let mut flags = text.split(',').map(|flag| flag == "1");
        if text.is_empty() {
            return PowerState {
                charging: true,
                power_save: false,
                low: false,
            };
        }
        PowerState {
            charging: flags.next().unwrap_or(true),
            power_save: flags.next().unwrap_or(false),
            low: flags.next().unwrap_or(false),
        }
    }

    pub(super) fn watch_power() -> bool {
        with_jni_class(BATTERY, false, |env, class| {
            env.call_static_method(class, "watchPower", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn battery_is_exempt() -> bool {
        with_jni_class(BATTERY, false, |env, class| {
            env.call_static_method(class, "isExempt", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn request_battery_exemption() -> bool {
        with_jni_class(BATTERY, false, |env, class| {
            env.call_static_method(class, "requestExemption", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn open_battery_settings() -> bool {
        with_jni_class(BATTERY, false, |env, class| {
            env.call_static_method(class, "openSettings", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}
