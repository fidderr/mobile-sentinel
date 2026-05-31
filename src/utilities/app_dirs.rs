//! Platform-aware app directory resolution.

use std::path::PathBuf;

/// Get the app's internal files directory.
/// On Android: calls Context.getFilesDir() via JNI.
/// On other platforms: returns current directory.
#[cfg(target_os = "android")]
pub fn app_files_dir() -> PathBuf {
    use crate::platform::android::context::get_activity;

    // ndk_context only knows about the Activity in processes that
    // registered it (typically MAIN via the host mobile framework).
    // In `:sentinel` cold-start this returns null pointers; bail out
    // safely instead of segfaulting on `JObject::from_raw(null)`.
    let raw = ndk_context::android_context();
    if raw.vm().is_null() || raw.context().is_null() {
        return PathBuf::from(".");
    }

    let Ok((vm, activity)) = get_activity() else {
        return PathBuf::from(".");
    };
    let Ok(mut env) = vm.attach_current_thread() else {
        return PathBuf::from(".");
    };

    let files_dir = match env.call_method(activity.as_obj(), "getFilesDir", "()Ljava/io/File;", &[])
    {
        Ok(v) => match v.l() {
            Ok(obj) => obj,
            Err(_) => return PathBuf::from("."),
        },
        Err(_) => {
            let _ = env.exception_clear();
            return PathBuf::from(".");
        }
    };

    let path_str = match env.call_method(&files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
    {
        Ok(v) => match v.l() {
            Ok(obj) => obj,
            Err(_) => return PathBuf::from("."),
        },
        Err(_) => {
            let _ = env.exception_clear();
            return PathBuf::from(".");
        }
    };

    let jstr = jni::objects::JString::from(path_str);
    let result = match env.get_string(&jstr) {
        Ok(s) => PathBuf::from(String::from(s)),
        Err(_) => PathBuf::from("."),
    };
    result
}

#[cfg(not(target_os = "android"))]
pub fn app_files_dir() -> PathBuf {
    PathBuf::from(".")
}
