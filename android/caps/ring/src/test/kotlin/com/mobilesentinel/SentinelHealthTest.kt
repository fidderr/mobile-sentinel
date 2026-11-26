package com.mobilesentinel

import android.app.NotificationManager
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAlarmManager

/** The permission state the Rust side reads in `features::health`, on every Android version. */
@RunWith(RobolectricTestRunner::class)
class SentinelHealthTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()

    @Before
    fun setUp() {
        // A phone that has been up for an hour: the files these tests write are newer than the boot.
        org.robolectric.shadows.ShadowSystemClock.advanceBy(java.time.Duration.ofHours(1))
        SentinelPrimitives.init(context)
    }

    private fun snapshot() = JSONObject(SentinelHealth.snapshot("ch"))

    @Test
    @Config(sdk = [26, 28, 29, 30, 31, 33, 34, 36])
    fun everyFieldRustReadsIsThereOnEveryVersion() {
        val json = snapshot()
        for (key in listOf(
            "exact_alarms", "notifications", "full_screen", "overlay", "battery_unrestricted",
            "background_allowed", "kept_when_unused", "channel_importance", "total_silence", "boot_signal_missed",
        )) {
            assertTrue("missing $key in $json", json.has(key))
        }
    }

    /** Rust calls these by name as static methods: without @JvmStatic the JNI call ends the process. */
    @Test
    @Config(sdk = [28])
    fun everyEntryPointRustCallsIsStatic() {
        for (name in listOf("snapshot", "openSettings", "applies")) {
            val methods = SentinelHealth::class.java.methods.filter { it.name == name }
            assertTrue("no method $name", methods.isNotEmpty())
            assertTrue("$name is not static", methods.any { java.lang.reflect.Modifier.isStatic(it.modifiers) })
        }
    }

    /** The "kept when unused" setting is Android 11's: an app does not list a check for a setting the phone does not have. */
    @Test
    @Config(sdk = [28, 29])
    fun theUnusedSettingIsNotThereBelowAndroid11() {
        assertFalse(SentinelHealth.applies("kept_when_unused"))
        assertTrue(SentinelHealth.applies("overlay"))
        assertTrue(SentinelHealth.applies("exact_alarms"))
    }

    @Test
    @Config(sdk = [30, 36])
    fun theUnusedSettingIsThereFromAndroid11() {
        assertTrue(SentinelHealth.applies("kept_when_unused"))
        assertTrue(SentinelHealth.applies("overlay"))
    }

    @Test
    @Config(sdk = [26, 28, 30])
    fun exactAlarmsAreAlwaysOnBelowAndroid12() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        assertTrue(snapshot().getBoolean("exact_alarms"))
    }

    @Test
    @Config(sdk = [31, 36])
    fun exactAlarmsFollowThePermissionFromAndroid12() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        assertFalse(snapshot().getBoolean("exact_alarms"))
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        assertTrue(snapshot().getBoolean("exact_alarms"))
    }

    /** The full-screen permission only exists from Android 14; before that it is granted. */
    @Test
    @Config(sdk = [26, 28, 33])
    fun fullScreenIsGrantedBelowAndroid14() {
        assertTrue(snapshot().getBoolean("full_screen"))
    }

    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun notificationsFollowTheSwitch() {
        val nm = shadowOf(context.getSystemService(NotificationManager::class.java))
        nm.setNotificationsEnabled(false)
        assertFalse(snapshot().getBoolean("notifications"))
        nm.setNotificationsEnabled(true)
        assertTrue(snapshot().getBoolean("notifications"))
    }

    @Test
    @Config(sdk = [28, 33])
    fun anUnknownChannelHasImportanceMinusOne() {
        assertEquals(-1, snapshot().getInt("channel_importance"))
    }
}
