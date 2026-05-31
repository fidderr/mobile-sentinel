//! Calendar capability — read/create/delete events. Gated behind
//! `calendar` (READ_CALENDAR / WRITE_CALENDAR). Sole entry point. The JNI
//! calls into `com.mobilesentinel.SentinelCalendarPrimitives` live here.

use crate::error::SentinelError;
pub use crate::sink_types::CalendarEvent;

/// Events between `start` and `end`.
pub fn get_events(
    start: std::time::SystemTime,
    end: std::time::SystemTime,
) -> Result<Vec<CalendarEvent>, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let json =
            android::calendar_get_events(system_time_to_millis(start), system_time_to_millis(end));
        parse_calendar_events(&json).map_err(|_| SentinelError::unavailable("calendar::get_events"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (start, end);
        Err(SentinelError::unavailable("calendar::get_events"))
    }
}

/// Create an event. Returns Ok on success.
pub fn create_event(event: &CalendarEvent) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        let id = android::calendar_create_event(
            &event.title,
            event.description.as_deref(),
            system_time_to_millis(event.start_time),
            system_time_to_millis(event.end_time),
            event.location.as_deref(),
        );
        if id.is_empty() {
            Err(SentinelError::unavailable("calendar::create_event"))
        } else {
            Ok(())
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = event;
        Err(SentinelError::unavailable("calendar::create_event"))
    }
}

/// Delete an event by id.
pub fn delete_event(id: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::calendar_delete_event(id) {
            Ok(())
        } else {
            Err(SentinelError::unavailable("calendar::delete_event"))
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = id;
        Err(SentinelError::unavailable("calendar::delete_event"))
    }
}

#[cfg(target_os = "android")]
fn system_time_to_millis(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(target_os = "android")]
fn millis_to_system_time(ms: i64) -> std::time::SystemTime {
    if ms >= 0 {
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64)
    } else {
        std::time::UNIX_EPOCH
    }
}

/// Parse the calendar-events JSON array string emitted by the Kotlin helper.
#[cfg(target_os = "android")]
fn parse_calendar_events(json: &str) -> Result<Vec<CalendarEvent>, serde_json::Error> {
    #[derive(serde::Deserialize)]
    struct Raw {
        id: String,
        title: String,
        description: Option<String>,
        start: i64,
        end: i64,
        location: Option<String>,
    }
    let raws: Vec<Raw> = serde_json::from_str(json)?;
    Ok(raws
        .into_iter()
        .map(|r| CalendarEvent {
            id: r.id,
            title: r.title,
            description: r.description,
            start_time: millis_to_system_time(r.start),
            end_time: millis_to_system_time(r.end),
            location: r.location,
        })
        .collect())
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JObject, JString, JValue};
    use jni::sys::jlong;
    use jni::JNIEnv;

    const CALENDAR: &str = "com/mobilesentinel/SentinelCalendarPrimitives";

    /// Build a nullable Java String from `Option<&str>`: `None` → JNI null.
    fn opt_jstr<'a>(env: &mut JNIEnv<'a>, s: Option<&str>) -> Option<JObject<'a>> {
        match s {
            Some(v) => Some(jni_str(env, v)?.into()),
            None => Some(JObject::null()),
        }
    }

    /// Returns the raw JSON array string of events (caller parses).
    pub(super) fn calendar_get_events(start_millis: i64, end_millis: i64) -> String {
        with_jni_class(CALENDAR, "[]".to_owned(), |env, class| {
            let res = env
                .call_static_method(
                    class,
                    "getEvents",
                    "(JJ)Ljava/lang/String;",
                    &[
                        JValue::Long(start_millis as jlong),
                        JValue::Long(end_millis as jlong),
                    ],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }

    /// Returns the created event id, or empty string on failure.
    pub(super) fn calendar_create_event(
        title: &str,
        description: Option<&str>,
        start_millis: i64,
        end_millis: i64,
        location: Option<&str>,
    ) -> String {
        with_jni_class(CALENDAR, String::new(), |env, class| {
            let s_title = jni_str(env, title)?;
            let s_desc = opt_jstr(env, description)?;
            let s_loc = opt_jstr(env, location)?;
            let res = env
                .call_static_method(
                    class,
                    "createEvent",
                    "(Ljava/lang/String;Ljava/lang/String;JJLjava/lang/String;)Ljava/lang/String;",
                    &[
                        JValue::Object(&s_title.into()),
                        JValue::Object(&s_desc),
                        JValue::Long(start_millis as jlong),
                        JValue::Long(end_millis as jlong),
                        JValue::Object(&s_loc),
                    ],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }

    pub(super) fn calendar_delete_event(id: &str) -> bool {
        with_jni_class(CALENDAR, false, |env, class| {
            let s_id = jni_str(env, id)?;
            env.call_static_method(
                class,
                "deleteEvent",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s_id.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }
}
