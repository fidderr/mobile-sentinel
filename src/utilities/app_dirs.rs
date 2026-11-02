//! Platform-aware app directory resolution.

use std::path::PathBuf;

/// Get the app's internal files directory.
/// On Android: calls Context.getFilesDir() via JNI.
/// On other platforms: returns current directory.
///
/// The directory never changes during a process's life, so the first
/// successful lookup is cached. Without the cache every call did a full JNI
/// round trip (attach, new GlobalRef, two method calls, detach) — and callers
/// such as `jobs_dir()` and pollers hit it many times a second.
#[cfg(target_os = "android")]
pub fn app_files_dir() -> PathBuf {
    static CACHED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    if let Some(dir) = CACHED.get() {
        return dir.clone();
    }
    let dir = lookup_app_files_dir();
    // Don't cache the "." fallback (the app context is not there yet); a later
    // call may succeed.
    if dir.as_path() != std::path::Path::new(".") {
        let _ = CACHED.set(dir.clone());
    }
    dir
}

#[cfg(target_os = "android")]
fn lookup_app_files_dir() -> PathBuf {
    use crate::platform::android::jni::with_jni_class;

    let path = with_jni_class(
        "com/mobilesentinel/SentinelPrimitives",
        String::new(),
        |env, class| {
            let value = env
                .call_static_method(class, "getFilesDirPath", "()Ljava/lang/String;", &[])
                .ok()?
                .l()
                .ok()?;
            let path: String = env.get_string(&value.into()).ok()?.into();
            Some(path)
        },
    );
    if path.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(path)
    }
}

#[cfg(not(target_os = "android"))]
pub fn app_files_dir() -> PathBuf {
    PathBuf::from(".")
}
