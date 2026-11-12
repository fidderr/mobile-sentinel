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
     * restored later. `-1f` is the system's own brightness, `BRIGHTNESS_OVERRIDE_NONE`.
     */
    @Volatile
    private var savedBrightness: Float = -1f
    /** Whether [savedBrightness] holds a value: the system's own is negative too. */
    private var brightnessSaved = false

    /**
     * The live UI Activity, or the stored context if that is one, or null.
     * Window-level operations require an Activity; the app context usually isn't one.
     */
    private fun activityOrNull(): Activity? =
        SentinelActivityTracker.currentResumedActivity ?: SentinelPrimitives.getAppContext() as? Activity

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
                    if (!brightnessSaved) {
                        savedBrightness = lp.screenBrightness
                        brightnessSaved = true
                    }
                    // Callers may repeat this, and a change of attributes costs a layout.
                    if (lp.screenBrightness != clamped) {
                        lp.screenBrightness = clamped
                        activity.window.attributes = lp
                    }
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
     * brightness.
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
            activity.runOnUiThread {
                try {
                    val lp = activity.window.attributes
                    lp.screenBrightness = if (brightnessSaved) {
                        savedBrightness
                    } else {
                        WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
                    }
                    brightnessSaved = false
                    activity.window.attributes = lp
                } catch (e: Throwable) {
                    Log.w(TAG, "restoreBrightness(UI): ${e.message}", e)
                }
            }
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
    /**
     * No screenshots of this window, and a blank picture in the list of recent apps.
     * Safe fallback: false.
     */
    @JvmStatic
    fun setSecure(enabled: Boolean): Boolean {
        val activity = activityOrNull() ?: return false
        return try {
            activity.runOnUiThread {
                val flag = android.view.WindowManager.LayoutParams.FLAG_SECURE
                if (enabled) activity.window.addFlags(flag) else activity.window.clearFlags(flag)
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setSecure: ${e.message}")
            false
        }
    }

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
     * How far (in dp) the system bars + display cutout overlap the app's
     * content view, and then how far the keyboard does, as
     * `"top,bottom,left,right,keyboard"`; `""` when unavailable.
     *
     * Measured against where the content view actually sits on screen, so it
     * is right in both worlds: edge-to-edge (Android 15+ with targetSdk 35
     * forces it — content runs under the bars, overlap > 0) and legacy layouts
     * where the system already offsets content (overlap 0, no double padding).
     */
    @JvmStatic
    fun getSafeAreaInsets(): String {
        val activity = SentinelActivityTracker.currentResumedActivity
            ?: activityOrNull()
            ?: return ""
        return try {
            val content = activity.findViewById<android.view.View>(android.R.id.content) ?: return ""
            val insets = androidx.core.view.ViewCompat.getRootWindowInsets(content) ?: return ""
            val bars = insets.getInsets(
                androidx.core.view.WindowInsetsCompat.Type.systemBars() or
                    androidx.core.view.WindowInsetsCompat.Type.displayCutout()
            )
            val loc = IntArray(2)
            content.getLocationOnScreen(loc)
            val root = content.rootView
            val below = root.height - (loc[1] + content.height)
            val top = maxOf(0, bars.top - loc[1])
            val bottom = maxOf(0, bars.bottom - below)
            val keyboard = maxOf(0, insets.getInsets(androidx.core.view.WindowInsetsCompat.Type.ime()).bottom - below)
            val left = maxOf(0, bars.left - loc[0])
            val right = maxOf(0, bars.right - (root.width - (loc[0] + content.width)))
            val d = activity.resources.displayMetrics.density
            "${top / d},${bottom / d},${left / d},${right / d},${keyboard / d}"
        } catch (e: Throwable) {
            Log.w(TAG, "getSafeAreaInsets: ${e.message}", e)
            ""
        }
    }

    /**
     * Use dark system-bar icons (`light = true`, for light app backgrounds)
     * or light icons (`light = false`, for dark backgrounds).
     */
    @JvmStatic
    fun isSystemDark(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return true
        val night = c.resources.configuration.uiMode and android.content.res.Configuration.UI_MODE_NIGHT_MASK
        return night == android.content.res.Configuration.UI_MODE_NIGHT_YES
    }

    @JvmStatic
    fun fontScale(): Float {
        val c = SentinelPrimitives.getAppContext() ?: return 1f
        return c.resources.configuration.fontScale
    }

    @JvmStatic
    fun animationsOff(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            android.provider.Settings.Global.getFloat(
                c.contentResolver, android.provider.Settings.Global.ANIMATOR_DURATION_SCALE, 1f
            ) == 0f
        } catch (e: Exception) {
            false
        }
    }

    @JvmStatic
    fun uses24HourClock(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return true
        return android.text.format.DateFormat.is24HourFormat(c)
    }

    @JvmStatic
    fun setLightSystemBars(light: Boolean): Boolean {
        val activity = SentinelActivityTracker.currentResumedActivity
            ?: activityOrNull()
            ?: return false
        return try {
            activity.runOnUiThread {
                try {
                    val c = androidx.core.view.WindowCompat.getInsetsController(
                        activity.window,
                        activity.window.decorView,
                    )
                    c.isAppearanceLightStatusBars = light
                    c.isAppearanceLightNavigationBars = light
                } catch (e: Throwable) {
                    Log.w(TAG, "setLightSystemBars(UI): ${e.message}", e)
                }
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setLightSystemBars: ${e.message}", e)
            false
        }
    }

    /**
     * The colour of the system bars, as ARGB. Up to Android 14 the app does not draw under them
     * unless it asks to; from Android 15 on it always does and this changes nothing.
     */
    @JvmStatic
    fun setSystemBarColor(color: Int): Boolean {
        val activity = SentinelActivityTracker.currentResumedActivity
            ?: activityOrNull()
            ?: return false
        return try {
            activity.runOnUiThread {
                try {
                    @Suppress("DEPRECATION")
                    activity.window.apply {
                        clearFlags(
                            android.view.WindowManager.LayoutParams.FLAG_TRANSLUCENT_STATUS or
                                android.view.WindowManager.LayoutParams.FLAG_TRANSLUCENT_NAVIGATION,
                        )
                        addFlags(android.view.WindowManager.LayoutParams.FLAG_DRAWS_SYSTEM_BAR_BACKGROUNDS)
                        statusBarColor = color
                        navigationBarColor = color
                    }
                } catch (e: Throwable) {
                    Log.w(TAG, "setSystemBarColor(UI): ${e.message}", e)
                }
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setSystemBarColor: ${e.message}", e)
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
