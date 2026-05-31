package com.mobilesentinel

import android.app.Activity
import android.app.Application
import android.os.Bundle
import android.util.Log

/**
 * Core, capability-agnostic tracker of the currently-resumed [Activity].
 *
 * Any capability that needs an Activity reference from a non-Activity context
 * (runtime-permission dialogs, screen pinning, the scanner/file-picker launch,
 * the kiosk relaunch watchdog) reads it here. It lives in `:sentinel-core`
 * because it is pure plumbing with no alarm/kiosk semantics — the core library
 * initializer registers it during `Application.onCreate`.
 *
 * Holds only a reference to the live resumed Activity (cleared on pause), so
 * it never leaks a destroyed Activity.
 */
object SentinelActivityTracker : Application.ActivityLifecycleCallbacks {

    private const val TAG = "MobileSentinel.ActTrack"

    /** The currently-resumed Activity, or null when none is in the foreground. */
    @Volatile
    @JvmStatic
    var currentResumedActivity: Activity? = null
        private set

    /**
     * Timestamp (uptimeMillis) until which a capability has asked the kiosk
     * relaunch watchdog to stand down — e.g. the scanner / file picker
     * launches an internal Activity and does not want the kiosk to relaunch
     * MAIN over it. Core owns this flag (it is pure plumbing); the kiosk
     * controller, when present, reads it. A consumer with no kiosk simply
     * never has a watchdog to suppress, so the flag is harmless.
     */
    @Volatile
    @JvmStatic
    var suppressRelaunchUntilMs: Long = 0L
        private set

    /** Suppress kiosk relaunch for `durationMs` from now. */
    @JvmStatic
    fun suppressRelaunchFor(durationMs: Long) {
        suppressRelaunchUntilMs = android.os.SystemClock.uptimeMillis() + durationMs
    }

    /** Register on the host Application. Idempotent enough for one init call. */
    @JvmStatic
    fun attach(app: Application) {
        app.registerActivityLifecycleCallbacks(this)
    }

    /**
     * True when the activity whose class matches [fqcn] is the currently
     * resumed activity. Used by Rust (the firing sink) to decide whether a
     * full-screen intent / FGS banner is redundant because the firing UI is
     * already visible. Matches either the manifest component class or the
     * concrete Java class (Dioxus's WryActivity wraps the declared
     * MainActivity, so the two can differ).
     */
    @JvmStatic
    fun isActivityResumed(fqcn: String): Boolean {
        if (fqcn.isEmpty()) return false
        val a = currentResumedActivity ?: return false
        return a.componentName.className == fqcn || a.javaClass.name == fqcn
    }

    override fun onActivityResumed(activity: Activity) {
        currentResumedActivity = activity
    }

    override fun onActivityPaused(activity: Activity) {
        if (currentResumedActivity === activity) {
            currentResumedActivity = null
        }
    }

    override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}
    override fun onActivityStarted(activity: Activity) {}
    override fun onActivityStopped(activity: Activity) {}
    override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
    override fun onActivityDestroyed(activity: Activity) {
        if (currentResumedActivity === activity) {
            currentResumedActivity = null
        }
    }
}
