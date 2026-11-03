package com.mobilesentinel

import android.app.Activity
import android.app.Application
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher

/**
 * Activity-lock controller — keeps the firing activity in the foreground.
 *
 * Enabled/disabled from Rust (the firing sink) via
 * [SentinelKioskPrimitives.enableKioskMode] / `disableKioskMode`. ALL policy
 * is supplied by Rust through [enable] — the controller hardcodes no
 * behavioral choices, so other consumers can drive it differently (block back
 * but allow home, longer relaunch debounce, etc.). While `enabled = true`:
 * - `onActivityResumed(target)` asserts `setShowWhenLocked(true)` /
 *   `setTurnScreenOn(true)`, keeps the screen on, applies the immersive policy,
 *   excludes the task from recents, and registers a high-priority
 *   `OnBackInvokedCallback` that consumes Back (when `blockBack`). Volume keys
 *   are swallowed too (when `blockVolume`).
 * - `onActivityStopped(target)` relaunches the target (subject to
 *   `relaunchDebounceMs`) when `blockHome` / `blockRecents` is set. So does the
 *   target losing the window focus while it stays resumed: see [onTargetFocus].
 *
 * Relaunch is suppressed while a capability (camera scanner, file picker) is
 * launching an internal activity, via [SentinelActivityTracker]
 * (`suppressRelaunchFor`, in core) — capability modules depend on core, not on
 * this module, so the suppression flag lives there. This is the single source
 * of truth for relaunch suppression.
 *
 * This class is generic: it contains no alarm-specific strings. The activity
 * FQCN and all switches come in through [enable].
 */
object SentinelKioskController : Application.ActivityLifecycleCallbacks {

    private const val TAG = "SentinelKiosk"

    /** Cleared again in [disable]. */
    private const val KEEP_AWAKE_FLAGS = android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON

    /**
     * Long enough for what does get paused, a dialog or the scanner, to have been paused, and
     * for a turn of the screen on a slow device, which takes the focus away while it draws.
     */
    private const val NO_FOCUS_RELAUNCH_MS = 1_500L

    @Volatile var enabled: Boolean = false
    @Volatile var targetActivityFqcn: String = ""
    @Volatile var blockHome: Boolean = true
    @Volatile var blockBack: Boolean = true
    @Volatile var blockRecents: Boolean = true
    @Volatile var blockVolume: Boolean = true
    @Volatile var relaunchDebounceMs: Long = 200
    /** Whether to hide the status bar (clock/battery) while kiosk-firing. */
    @Volatile var hideStatusBar: Boolean = true
    /** Whether to hide the navigation bar while kiosk-firing. */
    @Volatile var hideNavBar: Boolean = true

    /** Currently resumed activity — used by helpers to launch internal activities in the same task. */
    @Volatile var currentResumedActivity: Activity? = null

    /** Set of currently started (visible) activity class names. */
    private val startedActivities: MutableSet<String> = mutableSetOf()

    /**
     * Returns true if the activity matches the configured target.
     * Uses componentName.className which reflects the manifest-declared class
     * (handles cases where the actual Java class is a subclass — e.g. Dioxus's
     * WryActivity wraps MainActivity).
     */
    private fun isTargetActivity(activity: Activity): Boolean {
        if (targetActivityFqcn.isEmpty()) return false
        return activity.componentName.className == targetActivityFqcn
            || activity.javaClass.name == targetActivityFqcn
    }

    /** Internal (non-target) activities currently alive in our process. */
    private val internalActivities: MutableSet<Activity> = java.util.Collections.newSetFromMap(
        java.util.WeakHashMap<Activity, Boolean>()
    )

    /** Cross-process kiosk binding state, owned by this module. */
    private const val KIOSK_STATE_FILE = "mobile_sentinel_kiosk_state"

    // ---- Cross-process kiosk-state file (owned here) ------------------------

    private fun writeKioskStateFile(ctx: Context) {
        try {
            val file = java.io.File(SentinelStorage.filesDir(ctx), KIOSK_STATE_FILE)
            file.writeText("$targetActivityFqcn\n$relaunchDebounceMs\n$hideStatusBar\n$hideNavBar\n$blockVolume")
        } catch (e: Throwable) {
            Log.w(TAG, "writeKioskStateFile failed: ${e.message}")
        }
    }

    private fun clearKioskStateFile(ctx: Context) {
        try {
            val file = java.io.File(SentinelStorage.filesDir(ctx), KIOSK_STATE_FILE)
            if (file.exists()) file.delete()
        } catch (e: Throwable) {
            Log.w(TAG, "clearKioskStateFile failed: ${e.message}")
        }
    }

    private data class KioskState(
        val fqcn: String,
        val relaunchDebounceMs: Long,
        val hideStatusBar: Boolean,
        val hideNavBar: Boolean,
        val blockVolume: Boolean,
    )

    private fun readKioskStateFile(ctx: Context): KioskState? {
        return try {
            val file = java.io.File(SentinelStorage.filesDir(ctx), KIOSK_STATE_FILE)
            if (!file.exists()) return null
            // Written before the current boot: that alarm died with the
            // reboot. Engaging the lock for it would trap the user in an
            // app that has nothing ringing.
            if (SentinelBoot.isFromBeforeBoot(file)) {
                Log.i(TAG, "ignoring kiosk state from before the boot")
                file.delete()
                return null
            }
            val lines = file.readText().lines()
            if (lines.size < 2) return null
            val fqcn = lines[0].trim()
            val debounce = lines[1].trim().toLongOrNull() ?: 50L
            val hideStatus = lines.getOrNull(2)?.trim()?.toBooleanStrictOrNull() ?: false
            val hideNav = lines.getOrNull(3)?.trim()?.toBooleanStrictOrNull() ?: false
            val blockVolume = lines.getOrNull(4)?.trim()?.toBooleanStrictOrNull() ?: true
            if (fqcn.isBlank()) null else KioskState(fqcn, debounce, hideStatus, hideNav, blockVolume)
        } catch (_: Throwable) {
            null
        }
    }

    private var lastRelaunchAtMs: Long = 0L
    private var appContext: Context? = null
    private val handler = android.os.Handler(android.os.Looper.getMainLooper())
    private var noFocusCheck: Runnable? = null
    private val backCallbacks: MutableMap<Int, OnBackInvokedCallback> = mutableMapOf()

    /**
     * Attach to the host `Application`. Called exactly once, from
     * [SentinelKioskInitializer.onCreate] (which runs during
     * `Application.onCreate` via the zero-arg `ContentProvider` hook).
     */
    fun attach(application: Application) {
        if (appContext != null) {
            Log.w(TAG, "attach() called again — ignoring (already attached)")
            return
        }
        appContext = application.applicationContext
        application.registerActivityLifecycleCallbacks(this)
        Log.i(TAG, "attached to Application")
    }

    /**
     * Enable activity lock with the given policy. Called from Rust via JNI
     * ([SentinelKioskPrimitives.enableKioskMode]). Every behavioral choice is a
     * parameter — the controller hardcodes none.
     */
    @JvmStatic
    fun enable(
        activityFqcn: String,
        blockHome: Boolean,
        blockBack: Boolean,
        blockRecents: Boolean,
        blockVolume: Boolean = true,
        relaunchDebounceMs: Long,
        hideStatusBar: Boolean = false,
        hideNavBar: Boolean = false,
    ) {
        this.targetActivityFqcn = activityFqcn
        this.blockHome = blockHome
        this.blockBack = blockBack
        this.blockRecents = blockRecents
        this.blockVolume = blockVolume
        this.relaunchDebounceMs = relaunchDebounceMs
        this.hideStatusBar = hideStatusBar
        this.hideNavBar = hideNavBar
        this.enabled = true
        appContext?.let { writeKioskStateFile(it) }
        // The target may already be on screen: the firing sink does not relaunch an
        // activity that is visible, so no resume event will come to lock it.
        currentResumedActivity?.let { a ->
            if (isTargetActivity(a)) a.runOnUiThread { engageOn(a) }
        }
        Log.i(
            TAG,
            "enabled — target=$activityFqcn home=$blockHome back=$blockBack recents=$blockRecents volume=$blockVolume debounce=${relaunchDebounceMs}ms hideStatus=$hideStatusBar hideNav=$hideNavBar",
        )
    }

    /** Idempotent — safe to call while already disabled. */
    @JvmStatic
    fun disable() {
        enabled = false
        lastRelaunchAtMs = 0L
        appContext?.let { clearKioskStateFile(it) }
        currentResumedActivity?.let { a ->
            a.runOnUiThread {
                try {
                    a.window.clearFlags(KEEP_AWAKE_FLAGS)
                } catch (e: Throwable) {
                    Log.w(TAG, "clearing the window flags failed: ${e.message}")
                }
            }
        }
        // Re-include in recents now that kiosk is released.
        try {
            val ctx = appContext
            if (ctx != null) {
                val am = ctx.getSystemService(android.app.ActivityManager::class.java)
                am?.appTasks?.firstOrNull()?.setExcludeFromRecents(false)
            }
        } catch (_: Throwable) {}
        Log.i(TAG, "disabled")
    }

    /**
     * Cross-process kiosk auto-engage. Rust's `enableKioskMode` writes the
     * active binding to a plain file on disk (NOT SharedPreferences, which
     * caches per-process). On every activity-lifecycle callback in MAIN we
     * re-read that file and bind kiosk if it indicates an active session.
     * Idempotent — when already enabled or the file is absent this is a no-op.
     */
    private fun maybeAutoEngageFromPrefs(activity: Activity) {
        if (enabled) return
        try {
            val state = readKioskStateFile(activity.applicationContext) ?: return
            Log.i(TAG, "auto-engaging kiosk from on-disk state: ${state.fqcn}")
            enable(
                activityFqcn = state.fqcn,
                blockHome = true,
                blockBack = true,
                blockRecents = true,
                blockVolume = state.blockVolume,
                relaunchDebounceMs = state.relaunchDebounceMs,
                hideStatusBar = state.hideStatusBar,
                hideNavBar = state.hideNavBar,
            )
        } catch (e: Throwable) {
            Log.w(TAG, "kiosk auto-engage check failed: ${e.message}")
        }
    }

    /**
     * Called by the ActivityLifecycleCallbacks when the target activity is
     * leaving the foreground. Relaunches the target from this same process,
     * throttled by `relaunchDebounceMs`.
     *
     * For Back-key dismissals the OnBackInvokedCallback consumes the event
     * before the activity can finish, so this path is never reached. For
     * Home / overview the activity is paused but not finishing, so the
     * relaunch is a clean REORDER_TO_FRONT. One that is finishing anyway has
     * no use for the throttle.
     */
    private fun onActivityLeaving(activity: Activity) {
        if (!enabled) return

        // If relaunch is temporarily suppressed (scanner / picker launching an
        // internal activity), skip. The suppression flag lives in core
        // (SentinelActivityTracker) — capability modules ask there since they
        // don't depend on this module. This is the single source of truth.
        if (SystemClock.uptimeMillis() < SentinelActivityTracker.suppressRelaunchUntilMs) {
            Log.v(TAG, "relaunch suppressed (internal activity launching)")
            return
        }

        if (activity.isFinishing) {
            Log.i(TAG, "onActivityLeaving — ${activity.javaClass.simpleName} finishing, relaunching target")
            relaunchTarget()
            return
        }

        val now = SystemClock.uptimeMillis()
        if (now - lastRelaunchAtMs < relaunchDebounceMs) {
            Log.v(TAG, "relaunch debounced")
            return
        }
        lastRelaunchAtMs = now
        Log.i(TAG, "onActivityLeaving — relaunching target (from ${activity.javaClass.simpleName})")
        relaunchTarget()
    }

    /**
     * Recents on a recent launcher leaves the target resumed and visible, as a tile in its list:
     * no pause and no stop comes, the window focus is all that goes. Anything that covers the
     * target in another way pauses it, and [onActivityStopped] or nothing should deal with that:
     * a permission dialog has to stay.
     */
    private fun onTargetFocus(activity: Activity, hasFocus: Boolean) {
        noFocusCheck?.let(handler::removeCallbacks)
        noFocusCheck = null
        if (hasFocus || !enabled || !(blockHome || blockRecents)) return
        noFocusCheck = Runnable {
            if (enabled && screenOn(activity) && !activity.hasWindowFocus() &&
                SentinelActivityTracker.isActivityResumed(targetActivityFqcn)
            ) {
                Log.i(TAG, "target on screen without the focus")
                onActivityLeaving(activity)
            }
        }.also { handler.postDelayed(it, NO_FOCUS_RELAUNCH_MS) }
    }

    /** Whether the screen is on. A target that went with the screen was not left. */
    private fun screenOn(ctx: Context): Boolean = try {
        (ctx.getSystemService(Context.POWER_SERVICE) as? android.os.PowerManager)?.isInteractive ?: true
    } catch (_: Throwable) {
        true
    }

    private fun relaunchTarget() {
        val ctx = appContext ?: run {
            Log.w(TAG, "relaunchTarget skipped — no Application context")
            return
        }
        if (targetActivityFqcn.isBlank()) return
        // It would keep covering the backup screen, the one way out.
        if (SentinelRing.uiFailed(ctx)) return
        // Somebody tried to leave, so somebody is there.
        SentinelRing.noteActivity(ctx)
        val intent = Intent().apply {
            component = ComponentName(ctx.packageName, targetActivityFqcn)
            // NO CLEAR_TOP — it destroys the existing instance, and with our
            // periodic watchdog resurrection that would kill the activity on
            // every tick. Use REORDER_TO_FRONT + SINGLE_TOP to bring the
            // existing instance forward without destroying it.
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or
                    Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or
                    Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        try {
            ctx.startActivity(intent)
            Log.i(TAG, "relaunch dispatched for $targetActivityFqcn")
        } catch (e: Exception) {
            Log.e(TAG, "relaunch failed: ${e.message}", e)
        }
    }

    // ---- Application.ActivityLifecycleCallbacks -------------------------------

    override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {
        // Cross-process kiosk auto-engage. Rust's `enableKioskMode` writes a
        // flag to disk; MAIN reads it on every activity created/started/resumed
        // so kiosk binds even if the main process was already alive when the
        // alarm fired.
        maybeAutoEngageFromPrefs(activity)

        // Track non-target activities so we can finish them when target resumes.
        // Must check AFTER auto-engage so targetActivityFqcn is populated.
        if (!isTargetActivity(activity)
            && activity.packageName == appContext?.packageName) {
            internalActivities.add(activity)
        }

        // Register back-consumption as early as possible — activityCreated
        // fires before the first predictive-back gesture can be delivered,
        // so we beat the OS to the dispatcher.
        if (!enabled) return
        if (!isTargetActivity(activity)) return
        installKeyGuard(activity)
        if (blockBack && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerBackCallback(activity)
        }
    }
    override fun onActivityStarted(activity: Activity) {
        // Track by componentName.className to match the manifest-declared class
        // (Dioxus's WryActivity wraps MainActivity — they have the same component name).
        startedActivities.add(activity.componentName.className)
        maybeAutoEngageFromPrefs(activity)
    }

    override fun onActivityResumed(activity: Activity) {
        currentResumedActivity = activity
        maybeAutoEngageFromPrefs(activity)
        if (!enabled) return
        if (!isTargetActivity(activity)) return
        engageOn(activity)
    }

    /**
     * Apply the lock to the target [activity] that is on screen: exclude it from
     * recents, immersive policy, show-when-locked, and consume Back. Runs when
     * the target resumes, and also when the lock is switched on while the
     * target is already showing (no resume event comes then). Idempotent.
     */
    private fun engageOn(activity: Activity) {
        if (!enabled) return

        // Target activity resumed — finish any lingering internal activities
        // (scanner, file picker) that may still be alive in the background.
        // Iterate over a snapshot to avoid concurrent modification.
        val toFinish = internalActivities.toList()
        for (other in toFinish) {
            // Defensive: re-check that this activity isn't actually the target.
            // (Could have been added before kiosk engaged.)
            if (isTargetActivity(other)) {
                internalActivities.remove(other)
                continue
            }
            try {
                if (!other.isFinishing && !other.isDestroyed) {
                    Log.i(TAG, "finishing lingering internal activity: ${other.javaClass.simpleName}")
                    other.finish()
                }
            } catch (e: Throwable) {
                Log.w(TAG, "failed to finish ${other.javaClass.simpleName}: ${e.message}")
            }
        }

        // Exclude from Recents so the user cannot swipe the task away.
        // This is how Alarmy prevents swipe-to-dismiss: the task simply
        // doesn't appear in the recents list while the alarm is firing.
        try {
            val am = activity.getSystemService(android.app.ActivityManager::class.java)
            am?.appTasks?.firstOrNull()?.setExcludeFromRecents(true)
        } catch (e: Exception) {
            Log.w(TAG, "setExcludeFromRecents failed: ${e.message}")
        }

        // Immersive fullscreen — hide status bar and navigation bar so the
        // FGS notification doesn't show on top of the alarm UI. Android shows
        // them again temporarily on swipe-down.
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                val window = activity.window
                val controller = window.insetsController
                if (hideStatusBar || hideNavBar) {
                    window.setDecorFitsSystemWindows(false)
                    var mask = 0
                    if (hideStatusBar) mask = mask or android.view.WindowInsets.Type.statusBars()
                    if (hideNavBar) mask = mask or android.view.WindowInsets.Type.navigationBars()
                    controller?.hide(mask)
                    controller?.systemBarsBehavior =
                        android.view.WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                    // Make sure any bar we are NOT hiding is shown.
                    val showMask = (android.view.WindowInsets.Type.statusBars()
                            or android.view.WindowInsets.Type.navigationBars()) and mask.inv()
                    if (showMask != 0) controller?.show(showMask)
                } else {
                    window.setDecorFitsSystemWindows(true)
                    controller?.show(
                        android.view.WindowInsets.Type.statusBars()
                                or android.view.WindowInsets.Type.navigationBars()
                    )
                }
            } else {
                @Suppress("DEPRECATION")
                if (hideStatusBar || hideNavBar) {
                    var flags = android.view.View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
                            android.view.View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    if (hideStatusBar) {
                        flags = flags or android.view.View.SYSTEM_UI_FLAG_FULLSCREEN or
                                android.view.View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    }
                    if (hideNavBar) {
                        flags = flags or android.view.View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                                android.view.View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    }
                    activity.window.decorView.systemUiVisibility = flags
                } else {
                    activity.window.decorView.systemUiVisibility = 0
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "immersive mode failed: ${e.message}")
        }

        // Runtime assertion — the manifest attributes may not engage if the
        // activity reaches top via some OEM paths.
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
                activity.setShowWhenLocked(true)
                activity.setTurnScreenOn(true)
            } else {
                @Suppress("DEPRECATION")
                activity.window.addFlags(
                    android.view.WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or
                        android.view.WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON,
                )
            }
            // A challenge can take longer than the screen timeout.
            activity.window.addFlags(KEEP_AWAKE_FLAGS)
        } catch (e: Exception) {
            Log.w(TAG, "keeping the alarm on screen failed: ${e.message}")
        }

        // Register a high-priority OnBackInvokedCallback to consume Back.
        // onActivityCreated registers the same callback with the same key,
        // so this call is idempotent (registerBackCallback checks the map
        // and returns early when already registered).
        installKeyGuard(activity)
        if (blockBack && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerBackCallback(activity)
        }
    }

    // Not onActivityLeaving yet: in onStopped the started activity count is accurate (the
    // new activity's onStarted has fired by then).
    override fun onActivityPaused(activity: Activity) {}

    override fun onActivityStopped(activity: Activity) {
        startedActivities.remove(activity.componentName.className)
        if (!enabled) return

        val isTarget = isTargetActivity(activity)
        val isInternalActivity = activity.packageName == appContext?.packageName && !isTarget

        // Internal activity stopped (scanner closed). If target isn't visible,
        // user navigated away — relaunch IMMEDIATELY (bypass suppress).
        if (isInternalActivity && !isTargetInStartedSet()) {
            Log.i(TAG, "internal activity stopped, target not visible — relaunching immediately")
            // Bypass debounce — the scanner is gone, bring the target back now.
            lastRelaunchAtMs = 0L
            relaunchTarget()
            return
        }

        // Target activity stopped — normal relaunch flow
        if (isTarget && (blockRecents || blockHome)) {
            // The screen going off stops it as well. It stays on top for when the screen comes
            // back, and the ring service turns that on. Brought back now, it would sit resumed on a
            // dark screen with a page that does not draw, and so does not answer: that reads as
            // frozen, and ends in the backup screen for nothing.
            if (!screenOn(activity)) {
                Log.i(TAG, "target stopped with the screen off: it stays where it is")
                return
            }
            onActivityLeaving(activity)
        }
    }

    /** Check if the target activity is currently in the startedActivities set. */
    private fun isTargetInStartedSet(): Boolean {
        return startedActivities.contains(targetActivityFqcn)
    }

    override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}

    override fun onActivityDestroyed(activity: Activity) {
        internalActivities.remove(activity)
        if (currentResumedActivity === activity) currentResumedActivity = null
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            unregisterBackCallback(activity)
        }
    }

    // ---- Key handling: legacy KEYCODE_BACK (all API levels), volume keys -------
    //
    // The OnBackInvokedCallback below only sees Back when the system routes it
    // through predictive back. Some OEMs (seen on ColorOS: "predictive settings
    // is disabled") ignore the app's opt-in and deliver Back the legacy way — a
    // KEYCODE_BACK key event that ends in Activity.onBackPressed → finish. That
    // finish destroyed the alarm activity mid-ring. Wrapping the window callback
    // swallows that key event while the lock is on, and the volume keys as well;
    // the check is live, so the wrapper is inert once the kiosk is disabled.
    // It is also where the window focus can be seen without being the activity.

    private class KeyGuard(
        private val activity: Activity,
        private val base: android.view.Window.Callback,
    ) : android.view.Window.Callback by base {
        override fun onWindowFocusChanged(hasFocus: Boolean) {
            base.onWindowFocusChanged(hasFocus)
            SentinelKioskController.onTargetFocus(activity, hasFocus)
        }

        override fun dispatchKeyEvent(event: android.view.KeyEvent): Boolean {
            val controller = SentinelKioskController
            val blocked = when (event.keyCode) {
                android.view.KeyEvent.KEYCODE_BACK -> controller.blockBack
                android.view.KeyEvent.KEYCODE_VOLUME_UP,
                android.view.KeyEvent.KEYCODE_VOLUME_DOWN,
                android.view.KeyEvent.KEYCODE_VOLUME_MUTE,
                -> controller.blockVolume
                else -> false
            }
            if (blocked && controller.enabled) {
                if (event.action == android.view.KeyEvent.ACTION_UP) {
                    Log.d(TAG, "key ${event.keyCode} consumed by kiosk lock")
                }
                return true
            }
            return base.dispatchKeyEvent(event)
        }

        override fun dispatchTouchEvent(event: android.view.MotionEvent): Boolean {
            val controller = SentinelKioskController
            if (controller.enabled && event.actionMasked == android.view.MotionEvent.ACTION_DOWN) {
                controller.appContext?.let { SentinelRing.noteActivity(it) }
            }
            return base.dispatchTouchEvent(event)
        }
    }

    private fun installKeyGuard(activity: Activity) {
        try {
            val window = activity.window ?: return
            val current = window.callback ?: return
            if (current is KeyGuard) return
            window.callback = KeyGuard(activity, current)
            Log.d(TAG, "key guard installed")
        } catch (e: Throwable) {
            Log.w(TAG, "installKeyGuard failed: ${e.message}")
        }
    }

    // ---- Back-key handling (API 33+) -----------------------------------------

    @android.annotation.TargetApi(Build.VERSION_CODES.TIRAMISU)
    private fun registerBackCallback(activity: Activity) {
        val key = System.identityHashCode(activity)
        if (backCallbacks.containsKey(key)) return
        val dispatcher: OnBackInvokedDispatcher = activity.onBackInvokedDispatcher
        val cb = OnBackInvokedCallback {
            if (enabled && blockBack) {
                Log.d(TAG, "Back consumed by kiosk lock")
            }
        }
        try {
            dispatcher.registerOnBackInvokedCallback(
                OnBackInvokedDispatcher.PRIORITY_OVERLAY,
                cb,
            )
            backCallbacks[key] = cb
            Log.d(TAG, "OnBackInvokedCallback registered")
        } catch (e: Exception) {
            Log.w(TAG, "Failed to register OnBackInvokedCallback: ${e.message}")
        }
    }

    @android.annotation.TargetApi(Build.VERSION_CODES.TIRAMISU)
    private fun unregisterBackCallback(activity: Activity) {
        val key = System.identityHashCode(activity)
        val cb = backCallbacks.remove(key) ?: return
        try {
            activity.onBackInvokedDispatcher.unregisterOnBackInvokedCallback(cb)
        } catch (e: Exception) {
            Log.v(TAG, "Unregister OnBackInvokedCallback failed: ${e.message}")
        }
    }
}
