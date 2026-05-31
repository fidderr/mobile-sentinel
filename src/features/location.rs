//! Location capability — current device location. Gated behind `location`
//! (ACCESS_FINE/COARSE_LOCATION). Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelLocationPrimitives` live here.

use crate::error::SentinelError;
pub use crate::sink_types::Coordinate;

/// Current device location.
pub fn current() -> Result<Coordinate, SentinelError> {
    #[cfg(target_os = "android")]
    {
        match android::location_current() {
            Some((latitude, longitude)) => Ok(Coordinate {
                latitude,
                longitude,
            }),
            None => Err(SentinelError::unavailable("location::current")),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(SentinelError::unavailable("location::current"))
    }
}

/// Whether location services are enabled.
pub fn is_enabled() -> bool {
    #[cfg(target_os = "android")]
    {
        android::location_is_enabled()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;
    use jni::objects::JString;

    const LOCATION: &str = "com/mobilesentinel/SentinelLocationPrimitives";

    /// Parse a `"lat,lng"` string into `(f64, f64)`. Empty / malformed → None.
    fn parse_lat_lng(s: &str) -> Option<(f64, f64)> {
        if s.is_empty() {
            return None;
        }
        let (lat, lng) = s.split_once(',')?;
        Some((lat.trim().parse().ok()?, lng.trim().parse().ok()?))
    }

    /// Returns `(lat, lng)` parsed from the Kotlin `"lat,lng"` string.
    pub(super) fn location_current() -> Option<(f64, f64)> {
        let raw = with_jni_class(LOCATION, String::new(), |env, class| {
            let res = env
                .call_static_method(class, "getCurrent", "()Ljava/lang/String;", &[])
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        });
        parse_lat_lng(&raw)
    }

    pub(super) fn location_is_enabled() -> bool {
        with_jni_class(LOCATION, false, |env, class| {
            env.call_static_method(class, "isEnabled", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}
