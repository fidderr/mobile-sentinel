//! File-system capability — copy/list bundled APK assets. Gated behind
//! `file_system`. Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelFileSystemPrimitives` live here.

/// Copy a bundled APK asset to a filesystem destination. Returns `true`
/// on success.
pub fn copy_asset(asset_path: &str, dest_path: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        android::copy_asset(asset_path, dest_path)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (asset_path, dest_path);
        false
    }
}

/// List bundled APK assets under `path` (returns a JSON array string).
pub fn list_assets(path: &str) -> String {
    #[cfg(target_os = "android")]
    {
        android::list_assets(path)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = path;
        "[]".to_owned()
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JString, JValue};

    const FILE_SYSTEM: &str = "com/mobilesentinel/SentinelFileSystemPrimitives";

    pub(super) fn copy_asset(asset_path: &str, dest_path: &str) -> bool {
        with_jni_class(FILE_SYSTEM, false, |env, class| {
            let s_src = jni_str(env, asset_path)?;
            let s_dst = jni_str(env, dest_path)?;
            env.call_static_method(
                class,
                "copyAsset",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[JValue::Object(&s_src.into()), JValue::Object(&s_dst.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn list_assets(path: &str) -> String {
        with_jni_class(FILE_SYSTEM, String::new(), |env, class| {
            let s = jni_str(env, path)?;
            let res = env
                .call_static_method(
                    class,
                    "listAssets",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&s.into())],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }
}
