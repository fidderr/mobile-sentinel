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

    /** Whether any activity of this app is in front. */
    @JvmStatic
    fun isAnyActivityResumed(): Boolean = currentResumedActivity != null

    override fun onActivityResumed(activity: Activity) {
        currentResumedActivity = activity
        writeResumed(activity, APP_RESUMED_FILE, true)
        if (isMainActivity(activity)) {
            writeResumed(activity, MAIN_RESUMED_FILE, true)
        }
    }

    override fun onActivityPaused(activity: Activity) {
        if (currentResumedActivity === activity) {
            currentResumedActivity = null
            writeResumed(activity, APP_RESUMED_FILE, false)
        }
        if (isMainActivity(activity)) writeResumed(activity, MAIN_RESUMED_FILE, false)
    }

    /**
     * Plain file, present while the app's main (launcher) activity is on
     * screen; its content is `"<pid> <resume time ms>"`. Read by the `:sentinel`
     * process, which has no other way to tell a UI that is frozen on screen
     * from one that is simply not shown (screen off, scanner in front).
     */
    const val MAIN_RESUMED_FILE = "mobile_sentinel_main_resumed"

    /**
     * The same for any activity of the app: a scanner or a picker in front is the app's
     * screen too, and nothing should be put over it or shown about it.
     */
    const val APP_RESUMED_FILE = "mobile_sentinel_app_resumed"

    private fun isMainActivity(activity: Activity): Boolean {
        val launcher = activity.packageManager
            .getLaunchIntentForPackage(activity.packageName)?.component?.className
        return launcher != null && activity.componentName.className == launcher
    }

    private fun writeResumed(activity: Activity, name: String, resumed: Boolean) {
        try {
            val file = java.io.File(SentinelStorage.filesDir(activity), name)
            if (resumed) {
                // "<pid> <resume time ms>": a marker left behind by a killed
                // process names a pid that is no longer MAIN's.
                file.writeText("${android.os.Process.myPid()} ${System.currentTimeMillis()}")
            } else if (file.exists()) {
                file.delete()
            }
        } catch (e: Throwable) {
            Log.w(TAG, "writeResumed($name, $resumed) failed: ${e.message}")
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
