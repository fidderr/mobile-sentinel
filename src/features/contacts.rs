//! Contacts capability — read device contacts. Gated behind `contacts`
//! (READ_CONTACTS). Sole entry point.

use crate::error::SentinelError;
pub use crate::sink_types::Contact;

/// All device contacts.
pub fn get_all() -> Result<Vec<Contact>, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let json = android::contacts_get_all();
        parse_contacts(&json).map_err(|_| SentinelError::unavailable("contacts::get_all"))
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(SentinelError::unavailable("contacts::get_all"))
    }
}

/// Contacts matching `query`.
pub fn search(query: &str) -> Result<Vec<Contact>, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let json = android::contacts_search(query);
        parse_contacts(&json).map_err(|_| SentinelError::unavailable("contacts::search"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = query;
        Err(SentinelError::unavailable("contacts::search"))
    }
}

/// Whether the contacts read permission is granted.
pub fn has_permission() -> bool {
    #[cfg(target_os = "android")]
    {
        android::contacts_has_permission()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Parse the contacts JSON array string emitted by the Kotlin helper.
#[cfg(target_os = "android")]
fn parse_contacts(json: &str) -> Result<Vec<Contact>, serde_json::Error> {
    #[derive(serde::Deserialize)]
    struct Raw {
        id: String,
        display_name: String,
        phone_numbers: Vec<String>,
        email_addresses: Vec<String>,
    }
    let raws: Vec<Raw> = serde_json::from_str(json)?;
    Ok(raws
        .into_iter()
        .map(|r| Contact {
            id: r.id,
            display_name: r.display_name,
            phone_numbers: r.phone_numbers,
            email_addresses: r.email_addresses,
        })
        .collect())
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JString, JValue};

    const CONTACTS: &str = "com/mobilesentinel/SentinelContactsPrimitives";

    /// Returns the raw JSON array string of contacts (caller parses).
    pub(super) fn contacts_get_all() -> String {
        with_jni_class(CONTACTS, "[]".to_owned(), |env, class| {
            let res = env
                .call_static_method(class, "getAll", "()Ljava/lang/String;", &[])
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }

    pub(super) fn contacts_search(query: &str) -> String {
        with_jni_class(CONTACTS, "[]".to_owned(), |env, class| {
            let s_q = jni_str(env, query)?;
            let res = env
                .call_static_method(
                    class,
                    "search",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&s_q.into())],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }

    pub(super) fn contacts_has_permission() -> bool {
        with_jni_class(CONTACTS, false, |env, class| {
            env.call_static_method(class, "hasPermission", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}
