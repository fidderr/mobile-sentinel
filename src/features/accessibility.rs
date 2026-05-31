//! Accessibility capability — "ultra-protection" grant flow for the
//! `SentinelAccessibilityService` (which relaunches the kiosk-configured
//! activity if the user reaches Settings / power-menu / launcher while a
//! kiosk session is active).
//!
//! Gated behind the `accessibility` Cargo feature. POLICY-SENSITIVE on
//! Google Play (the service is declared only when this feature is enabled).
//! The consumer drives the grant flow with these two entry points; the
//! service itself is an OS-invoked manifest component (no JNI).
//!
//! ```toml
//! mobile-sentinel = { version = "...", features = ["accessibility"] }
//! ```

/// Whether the user has enabled `SentinelAccessibilityService` via
/// Settings → Accessibility. Returns `false` on host / any JNI failure
/// (safe default — callers treat it as "not enabled").
pub fn is_service_enabled() -> bool {
    #[cfg(target_os = "android")]
    {
        android::is_accessibility_service_enabled()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Open Android's Accessibility settings so the user can grant the service.
/// `Err` only if the Android Context can't be obtained; a missing Settings
/// activity is swallowed (`Ok(())`), matching the crate's other deep-links.
pub fn open_settings() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        android::open_accessibility_settings()
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("not supported on this platform".into())
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::context::get_activity;

    /// Reads `Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES` and checks
    /// whether `SentinelAccessibilityService`'s `ComponentName` appears in
    /// the colon-separated list.
    pub(super) fn is_accessibility_service_enabled() -> bool {
        let result = (|| -> Result<bool, String> {
            let (vm, activity) = get_activity().map_err(|e| format!("get_activity: {:?}", e))?;
            let mut env = vm
                .attach_current_thread()
                .map_err(|e| format!("attach: {}", e))?;

            // Activity.getContentResolver()
            let resolver = env
                .call_method(
                    activity.as_obj(),
                    "getContentResolver",
                    "()Landroid/content/ContentResolver;",
                    &[],
                )
                .map_err(|e| {
                    let _ = env.exception_clear();
                    format!("getContentResolver: {}", e)
                })?
                .l()
                .map_err(|e| format!("resolver.l: {}", e))?;

            // Settings.Secure.getString(resolver, "enabled_accessibility_services")
            let secure_cls = env
                .find_class("android/provider/Settings$Secure")
                .map_err(|e| {
                    let _ = env.exception_clear();
                    format!("find Settings$Secure: {}", e)
                })?;
            let key = env
                .new_string("enabled_accessibility_services")
                .map_err(|e| format!("new_string key: {}", e))?;
            let enabled_list = env
                .call_static_method(
                    secure_cls,
                    "getString",
                    "(Landroid/content/ContentResolver;Ljava/lang/String;)Ljava/lang/String;",
                    &[
                        jni::objects::JValue::Object(&resolver),
                        jni::objects::JValue::Object(&key),
                    ],
                )
                .map_err(|e| {
                    let _ = env.exception_clear();
                    format!("getString: {}", e)
                })?
                .l()
                .map_err(|e| format!("getString.l: {}", e))?;

            if enabled_list.is_null() {
                return Ok(false);
            }
            let jstr = jni::objects::JString::from(enabled_list);
            let list_str: String = env
                .get_string(&jstr)
                .map_err(|e| format!("get_string: {}", e))?
                .into();

            // Component name is `<packageName>/<service class name>`.
            let pkg_obj = env
                .call_method(
                    activity.as_obj(),
                    "getPackageName",
                    "()Ljava/lang/String;",
                    &[],
                )
                .map_err(|e| {
                    let _ = env.exception_clear();
                    format!("getPackageName: {}", e)
                })?
                .l()
                .map_err(|e| format!("pkg.l: {}", e))?;
            let pkg_str: String = env
                .get_string((&pkg_obj).into())
                .map_err(|e| format!("pkg.get_string: {}", e))?
                .into();

            let needle = format!(
                "{}/com.mobilesentinel.SentinelAccessibilityService",
                pkg_str
            );
            Ok(list_str
                .split(':')
                .any(|component| component.eq_ignore_ascii_case(&needle)))
        })();

        match result {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[accessibility] is_service_enabled failed: {}", e);
                false
            }
        }
    }

    pub(super) fn open_accessibility_settings() -> Result<(), String> {
        let (vm, activity) = get_activity().map_err(|e| format!("get_activity: {:?}", e))?;
        let mut env = vm
            .attach_current_thread()
            .map_err(|e| format!("attach: {}", e))?;

        let intent_class = env.find_class("android/content/Intent").map_err(|e| {
            let _ = env.exception_clear();
            format!("find Intent: {}", e)
        })?;
        let action = env
            .new_string("android.settings.ACCESSIBILITY_SETTINGS")
            .map_err(|e| format!("new_string action: {}", e))?;
        let intent = env
            .new_object(
                intent_class,
                "(Ljava/lang/String;)V",
                &[jni::objects::JValue::Object(&action)],
            )
            .map_err(|e| {
                let _ = env.exception_clear();
                format!("Intent(String) ctor: {}", e)
            })?;
        // FLAG_ACTIVITY_NEW_TASK = 0x10000000 — required because we're
        // launching from a potentially non-activity context.
        let _ = env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[jni::objects::JValue::Int(0x10000000)],
        );
        let _ = env.exception_clear();

        let _ = env.call_method(
            activity.as_obj(),
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[jni::objects::JValue::Object(&intent)],
        );
        let _ = env.exception_clear();
        log::info!("[accessibility] Accessibility settings launched");
        Ok(())
    }
}
