//! Shared JNI plumbing for the Android capability modules.
//!
//! This is the ONE place that knows how to talk to the JVM: cache the
//! `JavaVM`, resolve+cache a `com.mobilesentinel.*` class from any thread,
//! attach the current thread, and run a closure against a resolved class.
//!
//! It holds NO per-capability logic. Each capability's JNI calls live in its
//! own `crate::features::<cap>` module (and the firing surface in
//! `firing_sink_android`), which call [`with_jni_class`] + [`jni_str`] here.
//! Duplicating this plumbing per capability would be strictly worse, so it
//! stays shared; everything capability-specific is gated where it's used.

#![cfg(target_os = "android")]
// Shared JNI helpers: which helpers are actually called depends on the
// enabled feature set, so in a minimal build some are legitimately unused.
// Uncalled fns are dropped from the `.so` by linker DCE.
#![allow(dead_code)]

use jni::objects::{JClass, JString};
use jni::JNIEnv;
use once_cell::sync::OnceCell;

/// Cached `JavaVM`. Populated by [`super::callbacks::JNI_OnLoad`] (called by
/// the Android JVM as soon as the `.so` loads) so every JNI path can attach
/// a thread without going through `ndk_context` (which is only registered in
/// MAIN by the Dioxus mobile framework).
static JAVA_VM: OnceCell<jni::JavaVM> = OnceCell::new();

/// Cache of resolved `com/mobilesentinel/...` classes keyed by JNI binary
/// name, so each helper class is looked up once per process.
static HELPER_CLASSES: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashMap<&'static str, jni::objects::GlobalRef>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Set the cached JavaVM. Called once per process by
/// [`super::callbacks::JNI_OnLoad`].
pub(crate) fn set_java_vm(vm: jni::JavaVM) {
    let _ = JAVA_VM.set(vm);
}

/// Borrow the cached JavaVM, falling back to `ndk_context` if the cache
/// hasn't been seeded yet (host tests, MAIN before bridge init).
fn java_vm() -> Option<jni::JavaVM> {
    if let Some(vm) = JAVA_VM.get() {
        // SAFETY: the cached vm is valid for the lifetime of the process. We
        // can't move it out of the OnceCell, so wrap the raw pointer back
        // into a fresh `JavaVM` handle.
        let ptr = vm.get_java_vm_pointer();
        return unsafe { jni::JavaVM::from_raw(ptr).ok() };
    }
    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() {
        return None;
    }
    unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()
}

/// Resolve and cache a `com/mobilesentinel/...` class by its JNI binary name
/// (e.g. `"com/mobilesentinel/SentinelTorchPrimitives"`).
///
/// On Android the JVM thread that initialised the `.so` is usually a worker
/// thread; from there `FindClass` uses the *system* classloader which can't
/// see app DEX. We fall back to walking the activity's classloader (see
/// [`super::callbacks::load_app_class_global`]) to load app classes reliably
/// from any thread.
fn resolve_class(env: &mut JNIEnv, name: &'static str) -> Option<jni::objects::GlobalRef> {
    if let Some(c) = HELPER_CLASSES.lock().unwrap().get(name) {
        return Some(c.clone());
    }
    let resolved = if let Ok(cls) = env.find_class(name) {
        env.new_global_ref(cls).ok()
    } else {
        let _ = env.exception_clear();
        super::callbacks::load_app_class_global(name)
    };
    if let Some(global) = resolved {
        HELPER_CLASSES.lock().unwrap().insert(name, global.clone());
        return Some(global);
    }
    log::error!("[jni] could not load class {name}");
    None
}

/// Run `f` against a resolved `com/mobilesentinel/...` class on a JVM-attached
/// thread, returning `default` if the VM/class/closure is unavailable.
///
/// This is the single entry point every capability's JNI calls go through.
pub(crate) fn with_jni_class<F, R>(class_name: &'static str, default: R, f: F) -> R
where
    F: FnOnce(&mut JNIEnv, &JClass) -> Option<R>,
{
    let vm = match java_vm() {
        Some(v) => v,
        None => {
            log::error!("[jni] no JavaVM available");
            return default;
        }
    };
    let mut env = match vm.attach_current_thread_permanently() {
        Ok(e) => e,
        Err(e) => {
            log::error!("[jni] attach_current_thread failed: {:?}", e);
            return default;
        }
    };
    let class_ref = match resolve_class(&mut env, class_name) {
        Some(c) => c,
        None => return default,
    };
    let class: &JClass = <&JClass>::from(class_ref.as_obj());
    f(&mut env, class).unwrap_or(default)
}

/// Marshal a Rust `&str` into a JNI `JString`.
pub(crate) fn jni_str<'a>(env: &mut JNIEnv<'a>, s: &str) -> Option<JString<'a>> {
    env.new_string(s).ok()
}

/// Resolve a helper class for paths that manage their own thread attachment
/// (e.g. blocking calls that need a real Activity from `ndk_context`).
pub(crate) fn resolve_helper_class(
    env: &mut JNIEnv,
    name: &'static str,
) -> Option<jni::objects::GlobalRef> {
    resolve_class(env, name)
}
