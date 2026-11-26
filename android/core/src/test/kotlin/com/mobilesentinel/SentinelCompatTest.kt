package com.mobilesentinel

import android.app.Activity
import android.app.AlarmManager
import android.content.Context
import android.os.Build
import android.view.WindowManager
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAlarmManager

@Suppress("DEPRECATION")
@RunWith(RobolectricTestRunner::class)
class SentinelCompatTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val alarmManager get() = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager

    /** Below Android 12 there is no exact-alarm permission: the answer is yes whatever the OS says. */
    @Test
    @Config(sdk = [26, 28, 29, 30])
    fun exactAlarmsAreAllowedWithoutAPermissionBelow31() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        assertTrue(SentinelCompat.exactAlarmsAllowed(alarmManager))
    }

    @Test
    @Config(sdk = [31, 33, 34, 36])
    fun exactAlarmsFollowThePermissionFrom31() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        assertFalse(SentinelCompat.exactAlarmsAllowed(alarmManager))
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        assertTrue(SentinelCompat.exactAlarmsAllowed(alarmManager))
    }

    @Test
    @Config(sdk = [26, 28, 29, 30, 31, 33, 34, 36])
    fun everyVersionHasAVibrator() {
        assertNotNull(SentinelCompat.vibrator(context))
    }

    /** VibratorManager only exists from Android 12: below it the plain service has to be used. */
    @Test
    @Config(sdk = [26, 28, 30])
    fun belowAndroid12TheVibratorComesFromThePlainService() {
        assertEquals(null, context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE))
        assertNotNull(SentinelCompat.vibrator(context))
    }

    @Test
    @Config(sdk = [31, 36])
    fun fromAndroid12TheVibratorComesFromTheManager() {
        assertNotNull(context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE))
        assertNotNull(SentinelCompat.vibrator(context))
    }

    /** Android 8.0 has no Activity.setShowWhenLocked: the window flags do it. */
    @Test
    @Config(sdk = [26])
    fun android80UsesWindowFlagsOverTheLockScreen() {
        val activity = Robolectric.buildActivity(Activity::class.java).create().get()
        SentinelCompat.showOverLockScreen(activity)
        val flags = activity.window.attributes.flags
        assertTrue(flags and WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED != 0)
        assertTrue(flags and WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON != 0)
    }

    /** From 8.1 the Activity setters do it, and the deprecated window flags stay unused. */
    @Test
    @Config(sdk = [27, 28, 30, 33, 36])
    // No 34: Robolectric 4.18.1 cannot create an Activity there (ClassNotFoundException VirtualDeviceManager$VirtualDeviceListener, an API 35 class its shadow touches).
    fun android81AndUpUseTheActivitySetters() {
        val activity = Robolectric.buildActivity(Activity::class.java).create().get()
        SentinelCompat.showOverLockScreen(activity)
        assertTrue(Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1)
        assertEquals(0, activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED)
    }

    /** Rust sends an unset option as JSON null; Android's own optString would turn it into the text "null". */
    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun optStringTreatsJsonNullAsMissing() {
        val json = org.json.JSONObject("""{"a":null,"b":"x","n":7}""")
        assertEquals("", SentinelCompat.optString(json, "a"))
        assertEquals("fb", SentinelCompat.optString(json, "a", "fb"))
        assertEquals("", SentinelCompat.optString(json, "missing"))
        assertEquals("x", SentinelCompat.optString(json, "b"))
        assertEquals("7", SentinelCompat.optString(json, "n"))
        assertEquals("null", json.optString("a", ""))
    }
}
