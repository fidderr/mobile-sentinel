//! The event loop of a UI on Android.

/// How long a UI can stand still on a lost wake at most: too short to notice, and it does not
/// happen often. Each wake costs next to nothing, and none at all when the process is frozen.
#[cfg(target_os = "android")]
const WAKE_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// Keep the calling thread's event loop from sleeping through what was sent to it. Call it from
/// that thread, which for a Dioxus app is the root component.
///
/// tao only looks at what was sent to its loop when the looper reports a wake. A wake that
/// comes in together with a window event, as when another Activity closes, is reported as that
/// event alone. What was sent then stays in the queue until the next wake, and with Dioxus every
/// task and timer of the UI stands still until something like a touch comes along. So this
/// wakes the looper every so often, for as long as the process lives.
#[cfg(target_os = "android")]
pub fn keep_event_loop_awake() {
    use std::ffi::c_void;
    use std::sync::Once;

    #[link(name = "android")]
    extern "C" {
        fn ALooper_forThread() -> *mut c_void;
        fn ALooper_acquire(looper: *mut c_void);
        fn ALooper_wake(looper: *mut c_void);
    }

    struct Looper(*mut c_void);
    // SAFETY: all that is done with it is ALooper_wake, which any thread may call.
    unsafe impl Send for Looper {}
    impl Looper {
        fn wake(&self) {
            // SAFETY: acquired below and never released, so it stays valid.
            unsafe { ALooper_wake(self.0) };
        }
    }

    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        // SAFETY: takes no arguments, and null is handled.
        let looper = unsafe { ALooper_forThread() };
        if looper.is_null() {
            log::warn!("[MobileSentinel] keep_event_loop_awake: this thread has no event loop");
            return;
        }
        // SAFETY: a valid looper, just returned.
        unsafe { ALooper_acquire(looper) };
        let looper = Looper(looper);
        let spawned = std::thread::Builder::new()
            .name("event-loop-wake".into())
            .spawn(move || loop {
                std::thread::sleep(WAKE_EVERY);
                looper.wake();
            });
        if let Err(e) = spawned {
            log::warn!("[MobileSentinel] keep_event_loop_awake: {e}");
        }
    });
}

/// Only Android's event loop has this problem.
#[cfg(not(target_os = "android"))]
pub fn keep_event_loop_awake() {}
