//! What the phone allows, for an app that needs alarms to ring on time.
//!
//! No app can guarantee an alarm on every phone, so it should say what stands in the
//! way, and how to change it. The platform only reads the settings and opens the
//! right page; what counts as a problem is decided here.

use serde::Deserialize;

/// One thing the phone can allow or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Alarms can be set for an exact time.
    ExactAlarms,
    /// The app may post notifications. Without them there is no way into the alarm
    /// screen when the app cannot start it itself.
    Notifications,
    /// The alarm screen may come up over the lock screen.
    FullScreen,
    /// The alarm screen may come up over other apps.
    Overlay,
    /// Battery optimisation leaves the app alone.
    BatteryUnrestricted,
    /// The user has not restricted the app's background activity.
    BackgroundAllowed,
    /// Permissions are kept when the app has not been opened for a while.
    KeptWhenUnused,
    /// The alarm notification channel is loud enough to show.
    ChannelImportance,
    /// Do Not Disturb is not on Total Silence, which silences alarms too.
    NotTotalSilence,
}

impl Check {
    /// How the platform calls this check.
    fn key(self) -> &'static str {
        match self {
            Check::ExactAlarms => "exact_alarms",
            Check::Notifications => "notifications",
            Check::FullScreen => "full_screen",
            Check::Overlay => "overlay",
            Check::BatteryUnrestricted => "battery_unrestricted",
            Check::BackgroundAllowed => "background_allowed",
            Check::KeptWhenUnused => "kept_when_unused",
            Check::ChannelImportance => "channel_importance",
            Check::NotTotalSilence => "total_silence",
        }
    }
}

/// `NotificationManager.IMPORTANCE_HIGH`: below this a notification does not pop up.
const IMPORTANCE_HIGH: i32 = 4;

/// What the phone allows right now. A setting that could not be read counts as fine:
/// a warning that may be false teaches people to ignore warnings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct HealthReport {
    pub exact_alarms: bool,
    pub notifications: bool,
    pub full_screen: bool,
    pub overlay: bool,
    pub battery_unrestricted: bool,
    pub background_allowed: bool,
    pub kept_when_unused: bool,
    /// The channel's importance, -1 when it does not exist yet (the first ring
    /// creates it).
    pub channel_importance: i32,
    pub total_silence: bool,
    /// Lower case, for advice about what a phone maker changes (auto-start).
    pub manufacturer: String,
}

impl Default for HealthReport {
    fn default() -> Self {
        Self {
            exact_alarms: true,
            notifications: true,
            full_screen: true,
            overlay: true,
            battery_unrestricted: true,
            background_allowed: true,
            kept_when_unused: true,
            channel_importance: -1,
            total_silence: false,
            manufacturer: String::new(),
        }
    }
}

impl HealthReport {
    fn parse(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    /// Whether this check is fine.
    pub fn is_ok(&self, check: Check) -> bool {
        match check {
            Check::ExactAlarms => self.exact_alarms,
            Check::Notifications => self.notifications,
            Check::FullScreen => self.full_screen,
            Check::Overlay => self.overlay,
            Check::BatteryUnrestricted => self.battery_unrestricted,
            Check::BackgroundAllowed => self.background_allowed,
            Check::KeptWhenUnused => self.kept_when_unused,
            Check::ChannelImportance => {
                self.channel_importance < 0 || self.channel_importance >= IMPORTANCE_HIGH
            }
            Check::NotTotalSilence => !self.total_silence,
        }
    }

    /// Every check that is not fine, the ones that can silence an alarm first.
    pub fn problems(&self) -> Vec<Check> {
        [
            Check::ExactAlarms,
            Check::NotTotalSilence,
            Check::BackgroundAllowed,
            Check::BatteryUnrestricted,
            Check::Notifications,
            Check::FullScreen,
            Check::ChannelImportance,
            Check::Overlay,
            Check::KeptWhenUnused,
        ]
        .into_iter()
        .filter(|c| !self.is_ok(*c))
        .collect()
    }

    /// Whether this phone maker is known to stop apps in its own way, behind a
    /// setting no app can read.
    pub fn maker_restricts_background(&self) -> bool {
        const MAKERS: [&str; 10] = [
            "oneplus", "oppo", "realme", "xiaomi", "redmi", "poco", "huawei", "honor", "vivo",
            "samsung",
        ];
        MAKERS.contains(&self.manufacturer.as_str())
    }
}

/// Read what the phone allows. `channel_id` is the alarm notification channel.
pub fn report(channel_id: &str) -> HealthReport {
    #[cfg(target_os = "android")]
    {
        HealthReport::parse(&jni::snapshot(channel_id))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = channel_id;
        HealthReport::parse("{}")
    }
}

/// Open the system page where this check is changed. Returns whether one opened.
pub fn open_settings(check: Check, channel_id: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        jni::open_settings(check.key(), channel_id)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (check.key(), channel_id);
        false
    }
}

#[cfg(target_os = "android")]
mod jni {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use ::jni::objects::JValue;

    const HEALTH: &str = "com/mobilesentinel/SentinelHealth";

    pub(super) fn snapshot(channel_id: &str) -> String {
        with_jni_class(HEALTH, String::new(), |env, class| {
            let channel = jni_str(env, channel_id)?;
            let value = env
                .call_static_method(
                    class,
                    "snapshot",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&channel.into())],
                )
                .ok()?
                .l()
                .ok()?;
            let json: String = env.get_string(&value.into()).ok()?.into();
            Some(json)
        })
    }

    pub(super) fn open_settings(check: &str, channel_id: &str) -> bool {
        with_jni_class(HEALTH, false, |env, class| {
            let check = jni_str(env, check)?;
            let channel = jni_str(env, channel_id)?;
            env.call_static_method(
                class,
                "openSettings",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[
                    JValue::Object(&check.into()),
                    JValue::Object(&channel.into()),
                ],
            )
            .ok()?
            .z()
            .ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_could_not_be_read_counts_as_fine() {
        assert!(HealthReport::parse("{}").problems().is_empty());
        assert!(HealthReport::parse("not json").problems().is_empty());
    }

    #[test]
    fn the_ones_that_can_silence_an_alarm_come_first() {
        let report = HealthReport::parse(
            r#"{"overlay": false, "total_silence": true, "exact_alarms": false, "battery_unrestricted": false}"#,
        );
        assert_eq!(
            report.problems(),
            vec![
                Check::ExactAlarms,
                Check::NotTotalSilence,
                Check::BatteryUnrestricted,
                Check::Overlay
            ]
        );
    }

    #[test]
    fn a_channel_turned_down_is_a_problem_but_one_not_made_yet_is_not() {
        let importance = |n: i32| {
            HealthReport::parse(&format!(r#"{{"channel_importance": {n}}}"#))
                .is_ok(Check::ChannelImportance)
        };
        assert!(importance(-1));
        assert!(importance(5));
        assert!(importance(4));
        assert!(!importance(3));
        assert!(!importance(0));
    }

    #[test]
    fn a_phone_maker_that_stops_apps_is_known_by_its_lower_case_name() {
        let of = |m: &str| HealthReport {
            manufacturer: m.to_owned(),
            ..HealthReport::default()
        };
        assert!(of("oneplus").maker_restricts_background());
        assert!(!of("google").maker_restricts_background());
    }
}
