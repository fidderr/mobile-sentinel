//! Callback registration for Android system events.
//! Android delivers events (boot completed, job heads-up, sensor data) via
//! Kotlin BroadcastReceivers and Services that live inside mobile-sentinel's
//! AAR. Those Kotlin classes call JNI native functions, which dispatch to
//! callbacks registered here.
//! # Usage from app code
//! ```ignore
//! use mobile_sentinel::platform::android::callbacks;
//! callbacks::on_boot_completed(|| {
//! // Reschedule all alarms from DB
//! });
//! ```

#[cfg(target_os = "android")]
use std::sync::Mutex;

#[cfg(target_os = "android")]
use once_cell::sync::Lazy;

/// Type for boot completed callback.
#[cfg(target_os = "android")]
type BootCompletedCallback = Box<dyn Fn() + Send + Sync>;

/// Type for job heads-up callback: receives the job ID string.
#[cfg(target_os = "android")]
type JobHeadsUpCallback = Box<dyn Fn(String) + Send + Sync>;

/// Global callback storage.
#[cfg(target_os = "android")]
struct CallbackRegistry {
    boot_completed: Option<BootCompletedCallback>,
    job_heads_up: Option<JobHeadsUpCallback>,
}

#[cfg(target_os = "android")]
static CALLBACKS: Lazy<Mutex<CallbackRegistry>> = Lazy::new(|| {
    Mutex::new(CallbackRegistry {
        boot_completed: None,
        job_heads_up: None,
    })
});

/// Register a callback for device boot completed.
/// Called from SentinelBootReceiver. Use this to reschedule all alarms
/// since Android clears AlarmManager entries on reboot.
#[cfg(target_os = "android")]
pub fn on_boot_completed<F>(callback: F)
where
    F: Fn() + Send + Sync + 'static,
{
    CALLBACKS.lock().unwrap().boot_completed = Some(Box::new(callback));
}

/// Register a callback for when the job guardian sends a heads-up
/// broadcast while MAIN is alive. The callback receives the job ID.
/// Consumers use this to dispatch Fire for the alarm instance.
#[cfg(target_os = "android")]
pub fn on_job_heads_up<F>(callback: F)
where
    F: Fn(String) + Send + Sync + 'static,
{
    CALLBACKS.lock().unwrap().job_heads_up = Some(Box::new(callback));
}

// ---------------------------------------------------------------------------
// Classloader helpers — on Android the default JNI `FindClass` uses the
// system classloader from non-JNI-called threads, which cannot see app DEX.
// The only reliable way to load app classes from a worker thread is via the
// activity's classloader. These helpers centralise the pattern.
// ---------------------------------------------------------------------------

/// Load an app-side class by FQCN using the activity's `ClassLoader`.
/// Works from any thread — it fetches the activity via `context::get_activity`,
/// attaches the current thread, then calls `Activity.getClassLoader().loadClass(name)`.
/// On failure it clears any pending exception and returns `None`.
/// `class_fqcn` accepts either the slash form (`"com/mobilesentinel/Foo"`)
/// or the dotted form (`"com.mobilesentinel.Foo"`) — internal normalisation
/// converts to dots because `ClassLoader.loadClass` expects dots.
#[cfg(target_os = "android")]
pub(crate) fn load_app_class_global(class_fqcn: &str) -> Option<jni::objects::GlobalRef> {
    use crate::platform::android::context::get_activity;

    let (vm, activity) = get_activity().ok()?;
    let mut env = vm.attach_current_thread().ok()?;

    // Activity.getClassLoader()
    let classloader = env
        .call_method(
            activity.as_obj(),
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;

    // String.replace('/', '.') — FindClass uses '/', loadClass uses '.'
    let dotted = class_fqcn.replace('/', ".");
    let name_jstr = env.new_string(&dotted).ok()?;

    // ClassLoader.loadClass(String)
    let cls_obj = env.call_method(
        &classloader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[jni::objects::JValue::Object(&name_jstr)],
    );
    let cls_obj = match cls_obj {
        Ok(v) => v.l().ok()?,
        Err(_) => {
            let _ = env.exception_clear();
            return None;
        }
    };

    env.new_global_ref(cls_obj).ok()
}

// ---------------------------------------------------------------------------
// JNI native function implementations
// These are called from Kotlin via SentinelBridge.nativeOnXxx()
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// JNI_OnLoad — called by the Android JVM when the .so is loaded into a
// process (whether MAIN or `:sentinel`). We cache the JavaVM so every
// later JNI helper can attach without depending on ndk_context (which
// MAIN-only frameworks like Dioxus initialise).
// ---------------------------------------------------------------------------

#[cfg(target_os = "android")]
#[no_mangle]
#[allow(non_snake_case)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut std::ffi::c_void,
) -> jni::sys::jint {
    log::info!("[mobile-sentinel] JNI_OnLoad — caching JavaVM");
    let java_vm = match unsafe { jni::JavaVM::from_raw(vm) } {
        Ok(v) => v,
        Err(e) => {
            log::error!(
                "[mobile-sentinel] JNI_OnLoad: JavaVM::from_raw failed: {:?}",
                e
            );
            return jni::sys::JNI_VERSION_1_6;
        }
    };
    crate::platform::android::jni::set_java_vm(java_vm);
    jni::sys::JNI_VERSION_1_6
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_mobilesentinel_SentinelBridge_nativeOnBootCompleted(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    log::info!("[mobile-sentinel] Boot completed callback");

    if let Ok(registry) = CALLBACKS.lock() {
        if let Some(ref cb) = registry.boot_completed {
            cb();
        }
    }
}

/// JNI entry point for the job guardian's heads-up broadcast.
/// Called from MAIN process when the guardian detects MAIN is alive
/// and an active job needs attention.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_mobilesentinel_SentinelBridge_nativeOnJobHeadsUp(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    job_id: jni::objects::JString,
) {
    let id: String = env
        .get_string(&job_id)
        .map(|s| s.into())
        .unwrap_or_default();

    log::info!("[mobile-sentinel] Job heads-up callback: {}", id);

    if let Ok(registry) = CALLBACKS.lock() {
        if let Some(ref cb) = registry.job_heads_up {
            cb(id);
        }
    }
}

// ---------------------------------------------------------------------------
// Sensor JNI entry points
// ---------------------------------------------------------------------------

/// Called from Kotlin when accelerometer data arrives.
#[cfg(all(target_os = "android", feature = "sensors"))]
#[no_mangle]
pub extern "system" fn Java_com_mobilesentinel_SentinelBridge_nativeOnAccelerometerData(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    x: jni::sys::jdouble,
    y: jni::sys::jdouble,
    z: jni::sys::jdouble,
) {
    crate::platform::android::sensor_state::on_accelerometer_data(x, y, z);
}

/// Called from Kotlin when step count updates.
#[cfg(all(target_os = "android", feature = "sensors"))]
#[no_mangle]
pub extern "system" fn Java_com_mobilesentinel_SentinelBridge_nativeOnStepCount(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    steps: jni::sys::jint,
) {
    crate::platform::android::sensor_state::on_step_count(steps);
}
