package com.mobilesentinel

import android.app.Activity
import android.content.Context
import android.provider.Settings
import android.util.Log
import android.view.WindowManager

/**
 * Window-level display primitives: screen brightness + keep-screen-on + runtime
 * requested orientation.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around an Android `Window` / Activity API.
 *
 * IMPORTANT — brightness / keep-screen-on / orientation are Activity-window
 * operations. We prefer [SentinelActivityTracker.currentResumedActivity] for
 * the live UI Activity; we fall back to casting the app context (works in
 * some embed scenarios). If neither yields an Activity we log and return false.
 *
 * Threading: mutations run on the UI thread via [Activity.runOnUiThread].
 */
object SentinelDisplayPrimitives {
    private const val TAG = "MobileSentinel.Display"

    /** Maximum value of the system `SCREEN_BRIGHTNESS` setting (0..255). */
    private const val SYSTEM_BRIGHTNESS_MAX = 255.0f

    /**
     * Window brightness captured before the first override, so it can be
     * restored later. `-1f` means "nothing saved" (and also doubles as the
     * `BRIGHTNESS_OVERRIDE_NONE` sentinel value).
     */
    @Volatile
    private var savedBrightness: Float = -1f

    /**
     * Resolve the stored context as an [Activity], or null. Window-level
     * operations require an Activity; the app context usually isn't one.
     */
    private fun activityOrNull(): Activity? = SentinelPrimitives.getAppContext() as? Activity

    /**
     * Set the window brightness override. [level] is clamped to `0.0..1.0`.
     * Requires an Activity window. The previous brightness is captured into
     * [savedBrightness] before the first overwrite so [restoreBrightness]
     * can revert it.
     *
     * @return true on success, false if no Activity context is available.
     */
    @JvmStatic
    fun setBrightness(level: Float): Boolean {
        val activity = activityOrNull()
        if (activity == null) {
            Log.w(TAG, "setBrightness: brightness control requires an Activity window; app context is not an Activity")
            return false
        }
        return try {
            val clamped = level.coerceIn(0.0f, 1.0f)
            activity.runOnUiThread {
                try {
                    val lp = activity.window.attributes
                    // Capture the pre-override value once, before we clobber it.
                    if (savedBrightness < 0f) {
                        savedBrightness = lp.screenBrightness
                    }
                    lp.screenBrightness = clamped
                    activity.window.attributes = lp
                } catch (e: Throwable) {
                    Log.w(TAG, "setBrightness(UI): ${e.message}", e)
                }
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setBrightness: ${e.message}", e)
            false
        }
    }

    /**
     * Read the current brightness as `0.0..1.0`.
     *
     * Reads the system `SCREEN_BRIGHTNESS` setting (0..255) and normalizes
     * to `0.0..1.0`. This works without an Activity since it only needs a
     * `ContentResolver`.
     *
     * @return normalized brightness, or `-1.0f` on failure.
     */
    @JvmStatic
    fun getBrightness(): Float {
        val context: Context = SentinelPrimitives.getAppContext() ?: return -1.0f
        return try {
            val raw = Settings.System.getInt(
                context.contentResolver,
                Settings.System.SCREEN_BRIGHTNESS,
            )
            (raw / SYSTEM_BRIGHTNESS_MAX).coerceIn(0.0f, 1.0f)
        } catch (e: Throwable) {
            Log.w(TAG, "getBrightness: ${e.message}", e)
            -1.0f
        }
    }

    /**
     * Force the window to full brightness. Delegates to [setBrightness].
     *
     * @return true on success, false if no Activity context is available.
     */
    @JvmStatic
    fun setMaxBrightness(): Boolean = setBrightness(1.0f)

    /**
     * Restore the brightness captured by the first [setBrightness] call.
     * If a value was saved it is reapplied; otherwise the override is
     * cleared to `BRIGHTNESS_OVERRIDE_NONE` (-1f) to revert to the system
     * brightness. [savedBrightness] is reset afterwards.
     *
     * @return true on success, false if no Activity context is available.
     */
    @JvmStatic
    fun restoreBrightness(): Boolean {
        val activity = activityOrNull()
        if (activity == null) {
            Log.w(TAG, "restoreBrightness: brightness control requires an Activity window; app context is not an Activity")
            return false
        }
        return try {
            val target = if (savedBrightness >= 0f) {
                savedBrightness
            } else {
                WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
            }
            activity.runOnUiThread {
                try {
                    val lp = activity.window.attributes
                    lp.screenBrightness = target
                    activity.window.attributes = lp
                } catch (e: Throwable) {
                    Log.w(TAG, "restoreBrightness(UI): ${e.message}", e)
                }
            }
            savedBrightness = -1f
            true
        } catch (e: Throwable) {
            Log.w(TAG, "restoreBrightness: ${e.message}", e)
            false
        }
    }

    /**
     * Toggle `FLAG_KEEP_SCREEN_ON` on the Activity window. When [enabled]
     * the flag is added; otherwise it is cleared. Requires an Activity
     * window and runs on the UI thread.
     *
     * @return true on success, false if no Activity context is available.
     */
    @JvmStatic
    fun keepScreenOn(enabled: Boolean): Boolean {
        val activity = activityOrNull()
        if (activity == null) {
            Log.w(TAG, "keepScreenOn: keep-screen-on requires an Activity window; app context is not an Activity")
            return false
        }
        return try {
            activity.runOnUiThread {
                try {
                    if (enabled) {
                        activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                    } else {
                        activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                    }
                } catch (e: Throwable) {
                    Log.w(TAG, "keepScreenOn(UI): ${e.message}", e)
                }
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "keepScreenOn: ${e.message}", e)
            false
        }
    }

    /**
     * Set the Activity's requested screen orientation at runtime.
     * Corresponds to `Activity.setRequestedOrientation(int)`.
     * The int values are the ActivityInfo.SCREEN_ORIENTATION_* constants
     * (passed from Rust).
     */
    @JvmStatic
    fun setRequestedOrientation(orientation: Int): Boolean {
        val activity = SentinelActivityTracker.currentResumedActivity
            ?: activityOrNull()
        if (activity == null) {
            Log.w(TAG, "setRequestedOrientation: no Activity available (tracker or context cast)")
            return false
        }
        return try {
            activity.runOnUiThread {
                try {
                    activity.requestedOrientation = orientation
                } catch (e: Throwable) {
                    Log.w(TAG, "setRequestedOrientation(UI): ${e.message}", e)
                }
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setRequestedOrientation: ${e.message}", e)
            false
        }
    }

    /**
     * Read back `activity.requestedOrientation`.
     * Returns -100 when no Activity is available (sentinel for Rust side).
     */
    @JvmStatic
    fun getRequestedOrientation(): Int {
        val activity = SentinelActivityTracker.currentResumedActivity
            ?: activityOrNull()
        if (activity == null) {
            return -100
        }
        return try {
            activity.requestedOrientation
        } catch (e: Throwable) {
            Log.w(TAG, "getRequestedOrientation: ${e.message}", e)
            -100
        }
    }
}
