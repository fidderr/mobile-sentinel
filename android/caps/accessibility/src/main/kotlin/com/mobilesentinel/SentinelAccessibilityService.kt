package com.mobilesentinel

import android.accessibilityservice.AccessibilityService
import android.content.ComponentName
import android.content.Intent
import android.os.Build
import android.os.SystemClock
import android.util.Log
import android.view.accessibility.AccessibilityEvent

/**
 * Accessibility service that enforces Level-3 "ultra-protection" mode
 * while [SentinelKioskController] is enabled.
 * Why this exists
 * ---------------
 * `OnBackInvokedCallback` (API 33+) covers the Back gesture. The
 * `ActivityLifecycleCallbacks` relaunch path covers Home / Recents for
 * our own process. But there are two remaining escape hatches neither
 * can cover from inside the app process:
 * 1. **Settings → Apps → `<app>` → Force stop**. If the user opens
 * system Settings and force-stops us, the Activity is gone before we
 * can react.
 * 2. **Power menu** (long-press power button). The user can power off
 * or reboot, which also kills us.
 * An `AccessibilityService` runs in a separate process with `TYPE_WINDOW_STATE_CHANGED`
 * visibility across all apps. When the user opens one of the above
 * interfaces while the kiosk is active, we intercept and relaunch the
 * configured target activity, forcing the user back into the active session
 * screen.
 * Why it's opt-in
 * ---------------
 * Android flags `BIND_ACCESSIBILITY_SERVICE` to users as a high-privilege
 * permission and reviews apps that request it. We ship the service as
 * a **declared but disabled** component — the user explicitly toggles
 * "Ultra-protection" (or equivalent) in the host app's settings, which
 * opens `ACTION_ACCESSIBILITY_SETTINGS` where the user grants the
 * permission. Without that grant, the service is inert and
 * `onServiceConnected` is never called — normal users see no change in
 * behaviour.
 * The service is also gated at runtime by `SentinelKioskController.enabled`:
 * it only relaunches while the host app has an active kiosk session
 * (e.g. during an alarm firing). When kiosk is disabled, every event
 * is ignored.
 */
class SentinelAccessibilityService : AccessibilityService() {

    companion object {
        private const val TAG = "SentinelAccess"

        /**
 * Package names whose window appearance we treat as an escape
 * attempt while kiosk is engaged. Covers the Settings app, the
 * Android system UI (power menu, quick settings, Recents), and
 * the default launcher on AOSP / Pixel builds. OEMs with custom
 * launchers (Samsung OneUI, etc.) still resolve to the host's
 * `packageManager.getLaunchIntentForPackage(...)` fallback.
 */
        private val ESCAPE_PACKAGES: Set<String> = setOf(
            "com.android.settings",
            "com.android.systemui",
            "com.android.launcher",
            "com.android.launcher3",
            "com.google.android.apps.nexuslauncher",
        )

        /** Minimum interval between consecutive relaunches. */
        private const val RELAUNCH_DEBOUNCE_MS = 300L
    }

    @Volatile
    private var lastRelaunchAtMs: Long = 0L

    override fun onServiceConnected() {
        super.onServiceConnected()
        Log.i(TAG, "onServiceConnected — accessibility service active")
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        val e = event ?: return
        if (e.eventType != AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) return

 // Kiosk gate — we ONLY act when the host has explicitly engaged
 // the activity lock. Otherwise the service is a passive observer
 // and consumes zero battery beyond the system's normal event
 // dispatch.
        if (!SentinelKioskController.enabled) return

        val pkg = e.packageName?.toString().orEmpty()
        val targetPkg = applicationContext.packageName

 // Our own windows are allowed — including the firing screen.
        if (pkg == targetPkg) return

        if (pkg in ESCAPE_PACKAGES) {
            Log.w(TAG, "Escape attempt detected: pkg=$pkg class=${e.className} — relaunching target")
            relaunchTarget()
        }
    }

    override fun onInterrupt() {
 // Nothing to clean up — we don't hold any long-running state.
    }

    /**
 * Relaunch the activity registered with [SentinelKioskController].
 * Throttled by [RELAUNCH_DEBOUNCE_MS] to avoid feedback loops with
 * the system UI (which emits several window-state-changed events
 * during a single navigation).
 */
    private fun relaunchTarget() {
        val fqcn = SentinelKioskController.targetActivityFqcn
        if (fqcn.isBlank()) {
            Log.w(TAG, "relaunchTarget skipped — no target FQCN configured")
            return
        }
        val now = SystemClock.uptimeMillis()
        if (now - lastRelaunchAtMs < RELAUNCH_DEBOUNCE_MS) {
            Log.v(TAG, "relaunch debounced")
            return
        }
        lastRelaunchAtMs = now

 // On API 23+ we can also close the system dialogs (power menu,
 // quick settings) to remove the user's view of the escape path
 // immediately. Silent failure if the OEM doesn't honour it.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            try {
                performGlobalAction(GLOBAL_ACTION_HOME)
            } catch (_: Exception) {
            }
        }

        val ctx = applicationContext
        val intent = Intent().apply {
            component = ComponentName(ctx.packageName, fqcn)
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or
                    Intent.FLAG_ACTIVITY_CLEAR_TOP or
                    Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or
                    Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        try {
            ctx.startActivity(intent)
            Log.i(TAG, "relaunchTarget dispatched for $fqcn")
        } catch (e: Exception) {
            Log.e(TAG, "relaunchTarget failed: ${e.message}", e)
        }
    }
}
