package com.mobilesentinel

import android.app.ActivityManager
import android.content.Context
import android.util.Log

/**
 * Screen-pinning (lock task mode) primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around an Android API. Reached from Rust via `crate::features::screen_pin`.
 *
 * Screen pinning requires an Activity (`startLockTask` / `stopLockTask` are
 * Activity methods), so [pin] / [unpin] work on the resumed one, and return a
 * safe fallback when the app is not on screen.
 *
 * Every method is `@JvmStatic`, wrapped in try/catch, logs via [Log.w],
 * never throws, and returns a safe fallback.
 */
object SentinelScreenPinPrimitives {
    private const val TAG = "MobileSentinel.ScreenPin"

    /**
     * Starts lock task mode (screen pinning) on the current Activity.
     *
     * Returns true if `startLockTask` was invoked on an Activity.
     * Safe fallback: false (including when no Activity is resumed).
     */
    @JvmStatic
    fun pin(): Boolean {
        return try {
            val activity = SentinelActivityTracker.currentResumedActivity ?: run {
                Log.w(TAG, "pin: no resumed Activity")
                return false
            }
            activity.startLockTask()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "pin: ${e.message}")
            false
        }
    }

    /**
     * Stops lock task mode (screen pinning) on the current Activity.
     *
     * Returns true if `stopLockTask` was invoked on an Activity.
     * Safe fallback: false (including when no Activity is resumed).
     */
    @JvmStatic
    fun unpin(): Boolean {
        return try {
            val activity = SentinelActivityTracker.currentResumedActivity ?: run {
                Log.w(TAG, "unpin: no resumed Activity")
                return false
            }
            activity.stopLockTask()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "unpin: ${e.message}")
            false
        }
    }

    /**
     * Returns true when the device is currently in any lock task mode
     * (pinned or locked).
     *
     * Safe fallback: false.
     */
    @JvmStatic
    fun isPinned(): Boolean {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: run {
                Log.w(TAG, "isPinned: no app context")
                return false
            }
            val am = c.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
            am.lockTaskModeState != ActivityManager.LOCK_TASK_MODE_NONE
        } catch (e: Throwable) {
            Log.w(TAG, "isPinned: ${e.message}")
            false
        }
    }
}
