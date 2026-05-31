//! Network capability — connectivity status. Gated behind `network`
//! (ACCESS_NETWORK_STATE). Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelNetworkPrimitives` live here.

pub use crate::sink_types::ConnectionType;

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

    const NETWORK: &str = "com/mobilesentinel/SentinelNetworkPrimitives";

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
