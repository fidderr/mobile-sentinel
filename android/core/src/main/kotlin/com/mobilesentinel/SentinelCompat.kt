package com.mobilesentinel

import android.app.Activity
import android.app.AlarmManager
import android.content.Context
import android.os.Build
import android.os.Vibrator
import android.os.VibratorManager
import android.view.WindowManager
import org.json.JSONObject

/**
 * What differs between Android versions, in one place. The modules call these instead of an
 * API newer than the app's minimum SDK, which would throw NoSuchMethodError (an Error, so a
 * `catch (e: Exception)` does not stop it) on an older phone.
 */
object SentinelCompat {
    /** Below Android 12 exact alarms need no permission. */
    @JvmStatic
    fun exactAlarmsAllowed(am: AlarmManager): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S || am.canScheduleExactAlarms()

    /** The phone's vibrator, or null without one. */
    @JvmStatic
    @Suppress("DEPRECATION")
    fun vibrator(context: Context): Vibrator? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            (context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager)?.defaultVibrator
        } else {
            context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }

    /** Show over the lock screen and turn the screen on (Activity setters from Android 8.1, window flags before). */
    @JvmStatic
    @Suppress("DEPRECATION")
    fun showOverLockScreen(activity: Activity) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            activity.setShowWhenLocked(true)
            activity.setTurnScreenOn(true)
        } else {
            activity.window.addFlags(
                WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON
            )
        }
    }

    /**
     * A string of a JSON object, or [fallback] when it is missing or JSON null. Android's
     * `optString` turns a JSON null into the text "null", which Rust sends for an unset option.
     */
    @JvmStatic
    fun optString(json: JSONObject, name: String, fallback: String = ""): String =
        if (json.isNull(name)) fallback else json.optString(name, fallback)
}
