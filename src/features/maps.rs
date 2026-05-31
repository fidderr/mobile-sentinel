//! Maps / geocoding capability. Gated behind `maps`. Sole entry point. The
//! JNI calls into `com.mobilesentinel.SentinelMapsPrimitives` live here.

use crate::error::SentinelError;
pub use crate::sink_types::Coordinate;

/// Geocode an address string to coordinates.
pub fn geocode(address: &str) -> Result<Coordinate, SentinelError> {
    #[cfg(target_os = "android")]
    {
        match android::geocode(address) {
            Some((latitude, longitude)) => Ok(Coordinate {
                latitude,
                longitude,
            }),
            None => Err(SentinelError::unavailable("maps::geocode")),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = address;
        Err(SentinelError::unavailable("maps::geocode"))
    }
}

/// Reverse-geocode coordinates to an address string.
pub fn reverse_geocode(latitude: f64, longitude: f64) -> Result<String, SentinelError> {
    #[cfg(target_os = "android")]
    {
        match android::reverse_geocode(latitude, longitude) {
            Some(addr) => Ok(addr),
            None => Err(SentinelError::unavailable("maps::reverse_geocode")),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (latitude, longitude);
        Err(SentinelError::unavailable("maps::reverse_geocode"))
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JString, JValue};

    const MAPS: &str = "com/mobilesentinel/SentinelMapsPrimitives";

    /// Parse a `"lat,lng"` string into `(f64, f64)`. Empty / malformed → None.
    fn parse_lat_lng(s: &str) -> Option<(f64, f64)> {
        if s.is_empty() {
            return None;
        }
        let (lat, lng) = s.split_once(',')?;
        Some((lat.trim().parse().ok()?, lng.trim().parse().ok()?))
    }

    pub(super) fn geocode(address: &str) -> Option<(f64, f64)> {
        let raw = with_jni_class(MAPS, String::new(), |env, class| {
            let s_addr = jni_str(env, address)?;
            let res = env
                .call_static_method(
                    class,
                    "geocode",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&s_addr.into())],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        });
        parse_lat_lng(&raw)
    }

    pub(super) fn reverse_geocode(latitude: f64, longitude: f64) -> Option<String> {
        let raw = with_jni_class(MAPS, String::new(), |env, class| {
            let res = env
                .call_static_method(
                    class,
                    "reverseGeocode",
                    "(DD)Ljava/lang/String;",
                    &[JValue::Double(latitude), JValue::Double(longitude)],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        });
        if raw.is_empty() {
            None
        } else {
            Some(raw)
        }
    }
}
