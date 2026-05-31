package com.mobilesentinel

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.os.Build
import android.util.Log

/**
 * Screen-pinning (lock task mode) primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around an Android API. Reached from Rust via `primitives_ext`.
 *
 * Screen pinning requires an Activity (`startLockTask` / `stopLockTask`
 * are Activity methods). The application context returned by
 * [SentinelPrimitives.getAppContext] is generally NOT an Activity, so
 * [pin] / [unpin] only succeed when the stored context is an Activity;
 * otherwise they log a warning and return a safe fallback. The host must
 * call from an Activity context for pinning to work.
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
     * Safe fallback: false (including when the context is not an Activity).
     */
    @JvmStatic
    fun pin(): Boolean {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: run {
                Log.w(TAG, "pin: no app context")
                return false
            }
            val activity = c as? Activity
            if (activity == null) {
                Log.w(TAG, "pin: context is not an Activity; host must call from an Activity context")
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
     * Safe fallback: false (including when the context is not an Activity).
     */
    @JvmStatic
    fun unpin(): Boolean {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: run {
                Log.w(TAG, "unpin: no app context")
                return false
            }
            val activity = c as? Activity
            if (activity == null) {
                Log.w(TAG, "unpin: context is not an Activity; host must call from an Activity context")
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
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                am.lockTaskModeState != ActivityManager.LOCK_TASK_MODE_NONE
            } else {
                @Suppress("DEPRECATION")
                am.isInLockTaskMode
            }
        } catch (e: Throwable) {
            Log.w(TAG, "isPinned: ${e.message}")
            false
        }
    }
}
