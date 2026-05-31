package com.mobilesentinel

import android.util.Log

/**
 * Foregrounding capability — finish the app's activity (close after
 * dismiss/snooze).
 *
 * Lives in the `:sentinel-foregrounding` module, compiled only when the
 * `foregrounding` Cargo feature is enabled. Uses only the core
 * [SentinelActivityTracker] (no alarm/kiosk dependency). Callers that also
 * run a kiosk should disable it first, otherwise the kiosk relaunch watchdog
 * brings the activity straight back.
 *
 * Rust calls INTO this via JNI (`crate::features::foregrounding`).
 */
object SentinelForegroundingPrimitives {
    private const val TAG = "MobileSentinel.Fgnd"

    @JvmStatic
    fun finishActivity(): Boolean {
        val activity = SentinelActivityTracker.currentResumedActivity ?: run {
            Log.w(TAG, "finishActivity: no resumed Activity")
            return false
        }
        return try {
            activity.runOnUiThread {
                try {
                    activity.finishAndRemoveTask()
                    Log.i(TAG, "finishActivity: finishAndRemoveTask dispatched")
                } catch (e: Throwable) {
                    Log.w(TAG, "finishActivity dispatch failed: ${e.message}")
                }
            }
            true
        } catch (e: Throwable) {
            Log.e(TAG, "finishActivity: ${e.message}", e)
            false
        }
    }
}
