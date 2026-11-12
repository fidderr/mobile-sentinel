//! Display / brightness capability. Gated behind `display`.
//!
//! Screen brightness control + keep-screen-on. Sole entry point; not
//! reachable through any raw sink handle. The JNI calls into
//! `com.mobilesentinel.SentinelDisplayPrimitives` live here.

use crate::android_or;

/// Set screen brightness (0.0–1.0).
pub fn set_brightness(level: f32) {
    android_or!(
        {
            let _ = android::set_brightness(level);
        },
        {
            let _ = level;
        }
    )
}

/// Current screen brightness (0.0–1.0). Returns 0.0 on host; on Android a
/// negative value (-1.0) means "unknown / system default" per the platform.
pub fn brightness() -> f32 {
    android_or!(android::get_brightness(), 0.0)
}

/// Force maximum brightness.
pub fn set_max_brightness() {
    android_or!(
        {
            let _ = android::set_max_brightness();
        },
        ()
    )
}

/// Restore brightness to the pre-modification value.
pub fn restore_brightness() {
    android_or!(
        {
            let _ = android::restore_brightness();
        },
        ()
    )
}

/// Keep the screen on (or release).
pub fn keep_screen_on(enabled: bool) {
    android_or!(
        {
            let _ = android::keep_screen_on(enabled);
        },
        {
            let _ = enabled;
        }
    )
}

/// Screen orientation modes for [`set_requested_orientation`].
/// These map to the Android `ActivityInfo.SCREEN_ORIENTATION_*` constants
/// and can be used to lock the activity (e.g. `Portrait`) or allow the
/// device sensor to rotate the UI (e.g. `Unspecified` / `FullSensor`).
///
/// Use the build-time `screen_orientation` in `sentinel.toml` (or direct
/// `prepare_*_and_orientation`) to establish an app-wide default. Then call
/// these at runtime from individual screens (e.g. only the settings page
/// opts into rotation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScreenOrientation {
    /// Locked portrait (1). Typical default for phone UIs.
    Portrait,
    /// Locked landscape (0).
    Landscape,
    /// Sensor-driven portrait that permits 180° flips (7).
    SensorPortrait,
    /// Sensor-driven landscape (6).
    SensorLandscape,
    /// Full sensor freedom — any of the four rotations the device reports (10).
    FullSensor,
    /// Unspecified (-1): let the system decide (usually follows sensor / user
    /// rotation setting). This gives the "normal" rotating-with-the-phone
    /// behaviour.
    Unspecified,
    /// Ignore sensor; keep whatever the current orientation is (5).
    Nosensor,
    /// Match the activity behind this one (3).
    Behind,
    /// Respect the user's chosen rotation preference (2).
    User,
}

impl ScreenOrientation {
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn to_android_constant(self) -> i32 {
        match self {
            ScreenOrientation::Portrait => 1,
            ScreenOrientation::Landscape => 0,
            ScreenOrientation::SensorPortrait => 7,
            ScreenOrientation::SensorLandscape => 6,
            ScreenOrientation::FullSensor => 10,
            ScreenOrientation::Unspecified => -1,
            ScreenOrientation::Nosensor => 5,
            ScreenOrientation::Behind => 3,
            ScreenOrientation::User => 2,
        }
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn from_android_constant(code: i32) -> Option<Self> {
        match code {
            1 => Some(ScreenOrientation::Portrait),
            0 => Some(ScreenOrientation::Landscape),
            7 => Some(ScreenOrientation::SensorPortrait),
            6 => Some(ScreenOrientation::SensorLandscape),
            10 => Some(ScreenOrientation::FullSensor),
            -1 => Some(ScreenOrientation::Unspecified),
            5 => Some(ScreenOrientation::Nosensor),
            3 => Some(ScreenOrientation::Behind),
            2 => Some(ScreenOrientation::User),
            _ => None,
        }
    }
}

/// Request a runtime screen orientation for the current Activity.
///
/// This overrides whatever static `android:screenOrientation` (if any) was
/// declared in the manifest for the main activity. The override lasts until
/// the activity is recreated or another call changes it.
///
/// Common pattern for apps that want a portrait-locked UI except on one
/// screen (e.g. settings or a document viewer):
///
/// ```ignore
/// // on settings mount
/// mobile_sentinel::display::set_requested_orientation(
///     mobile_sentinel::display::ScreenOrientation::Unspecified
/// );
/// // on unmount / back navigation
/// mobile_sentinel::display::set_requested_orientation(
///     mobile_sentinel::display::ScreenOrientation::Portrait
/// );
/// ```
///
/// Requires the `display` feature. Safe no-op on non-Android and when no
/// Activity is available.
pub fn set_requested_orientation(orientation: ScreenOrientation) {
    android_or!(
        {
            let _ = android::set_requested_orientation(orientation);
        },
        {
            let _ = orientation;
        }
    )
}

/// Return the orientation last requested through this API (or that the
/// Activity currently reports via `getRequestedOrientation`). Returns `None`
/// if no Activity context is available.
pub fn current_requested_orientation() -> Option<ScreenOrientation> {
    android_or!(android::get_current_requested_orientation(), None)
}

/// How far the system bars and display cutout overlap the app's content, in
/// dp (= CSS px in a WebView). All zero when nothing overlaps.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SafeAreaInsets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    /// How far the keyboard covers the content from the bottom. An edge-to-edge window
    /// does not shrink for it, and nothing tells the page: re-query while it can show.
    pub keyboard: f32,
}

/// Current overlap between system bars/cutout and the app content.
///
/// Correct both edge-to-edge (Android 15+ with targetSdk 35 forces content
/// under the bars) and on older layouts where the system already offsets the
/// content (all zero). `None` when no Activity is available (and on host).
/// Re-query after rotation — the values change.
pub fn safe_area_insets() -> Option<SafeAreaInsets> {
    android_or!(parse_safe_area(&android::get_safe_area_insets()), None)
}

/// Whether the phone is set to its dark theme.
pub fn system_dark() -> bool {
    android_or!(android::flag("isSystemDark"), true)
}

/// The phone's font size setting as a factor (1.0 is the default size). 1.0 on host.
pub fn font_scale() -> f32 {
    android_or!(android::font_scale(), 1.0)
}

/// Whether the phone's animation scale is set to off (the "remove animations" option). `false`
/// on host.
pub fn animations_off() -> bool {
    android_or!(android::flag("animationsOff"), false)
}

/// The colour of the system bars as `0xAARRGGBB`, to go with the app's background where it does
/// not draw under them: up to Android 14. No-op on host.
pub fn set_system_bar_color(argb: u32) {
    android_or!(
        {
            let _ = android::set_system_bar_color(argb);
        },
        {
            let _ = argb;
        }
    )
}

/// Whether the phone is set to show the time as 24 hours.
pub fn uses_24_hour_clock() -> bool {
    android_or!(android::flag("uses24HourClock"), true)
}

/// No screenshots of the app's window, and a blank picture in the list of recent apps. No-op on host.
pub fn set_secure(enabled: bool) {
    android_or!(
        {
            let _ = android::set_secure(enabled);
        },
        {
            let _ = enabled;
        }
    )
}

/// Use dark system-bar icons (`true`, for light app backgrounds) or light
/// icons (`false`, for dark backgrounds). No-op on host.
pub fn set_light_system_bars(light: bool) {
    android_or!(
        {
            let _ = android::set_light_system_bars(light);
        },
        {
            let _ = light;
        }
    )
}

/// Parse `"top,bottom,left,right,keyboard"` from the Kotlin side.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn parse_safe_area(raw: &str) -> Option<SafeAreaInsets> {
    let mut it = raw.split(',').map(|v| v.trim().parse::<f32>().ok());
    let insets = SafeAreaInsets {
        top: it.next()??,
        bottom: it.next()??,
        left: it.next()??,
        right: it.next()??,
        keyboard: it.next()??,
    };
    let all = [
        insets.top,
        insets.bottom,
        insets.left,
        insets.right,
        insets.keyboard,
    ];
    all.iter()
        .all(|v| v.is_finite() && *v >= 0.0)
        .then_some(insets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kotlin_inset_string() {
        assert_eq!(
            parse_safe_area("24.0,48.5,0.0,0.0,290.0"),
            Some(SafeAreaInsets {
                top: 24.0,
                bottom: 48.5,
                left: 0.0,
                right: 0.0,
                keyboard: 290.0
            })
        );
    }

    #[test]
    fn rejects_empty_or_malformed() {
        assert_eq!(parse_safe_area(""), None);
        assert_eq!(parse_safe_area("1,2,3,4"), None);
        assert_eq!(parse_safe_area("a,b,c,d,e"), None);
        assert_eq!(parse_safe_area("-1,0,0,0,0"), None);
        assert_eq!(parse_safe_area("NaN,0,0,0,0"), None);
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;
    use jni::objects::JValue;

    const DISPLAY: &str = "com/mobilesentinel/SentinelDisplayPrimitives";

    pub(super) fn set_brightness(level: f32) -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(class, "setBrightness", "(F)Z", &[JValue::Float(level)])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn get_brightness() -> f32 {
        with_jni_class(DISPLAY, -1.0f32, |env, class| {
            env.call_static_method(class, "getBrightness", "()F", &[])
                .ok()
                .and_then(|v| v.f().ok())
        })
    }

    pub(super) fn set_max_brightness() -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(class, "setMaxBrightness", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn restore_brightness() -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(class, "restoreBrightness", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn keep_screen_on(enabled: bool) -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(
                class,
                "keepScreenOn",
                "(Z)Z",
                &[JValue::Bool(enabled as u8)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn set_requested_orientation(orientation: super::ScreenOrientation) -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(
                class,
                "setRequestedOrientation",
                "(I)Z",
                &[JValue::Int(orientation.to_android_constant())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn get_safe_area_insets() -> String {
        with_jni_class(DISPLAY, String::new(), |env, class| {
            let obj = env
                .call_static_method(class, "getSafeAreaInsets", "()Ljava/lang/String;", &[])
                .ok()?
                .l()
                .ok()?;
            let s: String = env.get_string(&obj.into()).ok()?.into();
            Some(s)
        })
    }

    pub(super) fn flag(method: &str) -> bool {
        with_jni_class(DISPLAY, true, |env, class| {
            env.call_static_method(class, method, "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn font_scale() -> f32 {
        with_jni_class(DISPLAY, 1.0f32, |env, class| {
            env.call_static_method(class, "fontScale", "()F", &[])
                .ok()
                .and_then(|v| v.f().ok())
        })
    }

    pub(super) fn set_secure(enabled: bool) -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(class, "setSecure", "(Z)Z", &[JValue::Bool(enabled as u8)])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn set_light_system_bars(light: bool) -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(
                class,
                "setLightSystemBars",
                "(Z)Z",
                &[JValue::Bool(light as u8)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn set_system_bar_color(argb: u32) -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(
                class,
                "setSystemBarColor",
                "(I)Z",
                &[JValue::Int(argb as i32)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn get_current_requested_orientation() -> Option<super::ScreenOrientation> {
        let raw = with_jni_class(DISPLAY, -100i32, |env, class| {
            env.call_static_method(class, "getRequestedOrientation", "()I", &[])
                .ok()
                .and_then(|v| v.i().ok())
        });
        if raw == -100 {
            return None;
        }
        super::ScreenOrientation::from_android_constant(raw)
    }
}
