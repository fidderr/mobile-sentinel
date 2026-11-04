//! Network capability — connectivity status. Gated behind `network`
//! (ACCESS_NETWORK_STATE). Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelNetworkPrimitives` live here.

pub use crate::sink_types::ConnectionType;

/// The default network, as Android reports it when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkState {
    pub kind: ConnectionType,
    /// Paid by the byte, as far as the phone knows.
    pub metered: bool,
}

/// `callback` hears of every change of the default network, also from one Wi-Fi to another. It
/// runs on a system thread: pass the news on. No-op on host.
pub fn on_change<F>(callback: F)
where
    F: Fn(NetworkState) + Send + Sync + 'static,
{
    #[cfg(target_os = "android")]
    {
        *android::ON_CHANGE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(callback));
        let _ = android::watch();
    }
    #[cfg(not(target_os = "android"))]
    let _ = callback;
}

/// `callback` hears when the radio went active for any app: sending now costs little extra
/// battery. It runs on a system thread: pass the news on. No-op on host.
pub fn on_radio_active<F>(callback: F)
where
    F: Fn() + Send + Sync + 'static,
{
    #[cfg(target_os = "android")]
    {
        *android::ON_RADIO
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(callback));
        let _ = android::watch();
    }
    #[cfg(not(target_os = "android"))]
    let _ = callback;
}

/// Whether the device currently has a network connection.
pub fn is_connected() -> bool {
    #[cfg(target_os = "android")]
    {
        android::network_is_connected()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// The current connection type (Wifi / Cellular / None).
pub fn connection_type() -> ConnectionType {
    #[cfg(target_os = "android")]
    {
        match android::network_connection_type() {
            1 => ConnectionType::Wifi,
            2 => ConnectionType::Cellular,
            _ => ConnectionType::None,
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        ConnectionType::None
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    use super::{ConnectionType, NetworkState};
    use std::sync::Mutex;

    const NETWORK: &str = "com/mobilesentinel/SentinelNetworkPrimitives";

    type ChangeCallback = Box<dyn Fn(NetworkState) + Send + Sync>;
    type RadioCallback = Box<dyn Fn() + Send + Sync>;
    pub(super) static ON_CHANGE: Mutex<Option<ChangeCallback>> = Mutex::new(None);
    pub(super) static ON_RADIO: Mutex<Option<RadioCallback>> = Mutex::new(None);

    #[no_mangle]
    pub extern "system" fn Java_com_mobilesentinel_SentinelNetworkPrimitives_nativeOnChange(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
        kind: jni::sys::jint,
        metered: jni::sys::jboolean,
    ) {
        let kind = match kind {
            1 => ConnectionType::Wifi,
            2 => ConnectionType::Cellular,
            3 => ConnectionType::Other,
            _ => ConnectionType::None,
        };
        if let Ok(callback) = ON_CHANGE.lock() {
            if let Some(callback) = callback.as_ref() {
                callback(NetworkState {
                    kind,
                    metered: metered != 0,
                });
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_mobilesentinel_SentinelNetworkPrimitives_nativeOnRadioActive(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
    ) {
        if let Ok(callback) = ON_RADIO.lock() {
            if let Some(callback) = callback.as_ref() {
                callback();
            }
        }
    }

    pub(super) fn watch() -> bool {
        with_jni_class(NETWORK, false, |env, class| {
            env.call_static_method(class, "watch", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn network_is_connected() -> bool {
        with_jni_class(NETWORK, false, |env, class| {
            env.call_static_method(class, "isConnected", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    /// Returns 0 = None, 1 = Wifi, 2 = Cellular.
    pub(super) fn network_connection_type() -> i32 {
        with_jni_class(NETWORK, 0, |env, class| {
            env.call_static_method(class, "connectionType", "()I", &[])
                .ok()
                .and_then(|v| v.i().ok())
        })
    }
}
