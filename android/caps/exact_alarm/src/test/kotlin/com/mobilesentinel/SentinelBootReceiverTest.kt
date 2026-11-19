package com.mobilesentinel

import android.content.Intent
import android.os.UserManager
import com.mobilesentinel.ExactAlarmTestSupport.context
import com.mobilesentinel.ExactAlarmTestSupport.forgetAllOsAlarms
import com.mobilesentinel.ExactAlarmTestSupport.scheduled
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAlarmManager

/** After a reboot Android has forgotten every alarm: the receiver arms them again from the record. */
@RunWith(RobolectricTestRunner::class)
class SentinelBootReceiverTest {
    private val now = System.currentTimeMillis()

    @Before
    fun setUp() {
        // A phone that has been up for an hour: the files these tests write are newer than the boot.
        org.robolectric.shadows.ShadowSystemClock.advanceBy(java.time.Duration.ofHours(1))
        SentinelPrimitives.init(context)
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
    }

    private fun reboot(action: String) {
        forgetAllOsAlarms()
        SentinelBootReceiver().onReceive(context, Intent(action))
    }

    private fun armedTimes() = scheduled().associate {
        shadowOf(it.operation).savedIntent.getStringExtra(SentinelAlarmReceiver.EXTRA_INSTANCE_ID) to
            (it.alarmClockInfo?.triggerTime ?: it.triggerAtMs)
    }

    @Test
    @Config(sdk = [26, 28, 30, 31, 33, 34, 36])
    fun aFutureAlarmComesBackAtItsTimeAfterBoot() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("future", now + 3_600_000, """{"title":"t"}""")
        reboot(Intent.ACTION_BOOT_COMPLETED)
        assertEquals(mapOf<String?, Long>("future" to now + 3_600_000), armedTimes())
        val again = shadowOf(scheduled().single().operation).savedIntent
        assertEquals("""{"title":"t"}""", again.getStringExtra(SentinelAlarmReceiver.EXTRA_METADATA_JSON))
    }

    @Test
    @Config(sdk = [33])
    fun everyBootActionReArms() {
        for (action in listOf(
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_LOCKED_BOOT_COMPLETED,
            "android.intent.action.QUICKBOOT_POWERON",
        )) {
            SentinelExactAlarmPrimitives.scheduleExactAlarm("x", now + 3_600_000, null)
            reboot(action)
            assertEquals("for $action", 1, scheduled().size)
        }
    }

    @Test
    @Config(sdk = [33])
    fun otherBroadcastsDoNothing() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("x", now + 3_600_000, null)
        reboot(Intent.ACTION_TIME_CHANGED)
        assertTrue(scheduled().isEmpty())
    }

    /** The phone was off through the alarm's time: it still rings, a second after boot, if its plan allows. */
    @Test
    @Config(sdk = [26, 33, 36])
    fun anAlarmThatPassedWhileOffStillRingsInsideItsLateWindow() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("late", now - 60_000, """{"late_ring_window_ms":600000}""")
        reboot(Intent.ACTION_BOOT_COMPLETED)
        val at = armedTimes().getValue("late")
        assertTrue("rings about a second after boot, not at its old time: $at", at >= now && at <= now + 10_000)
    }

    @Test
    @Config(sdk = [26, 33, 36])
    fun anAlarmTooLateIsDroppedFromTheRecord() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("old", now - 3_600_000, """{"late_ring_window_ms":600000}""")
        SentinelExactAlarmPrimitives.scheduleExactAlarm("nowindow", now - 1_000, null)
        reboot(Intent.ACTION_BOOT_COMPLETED)
        assertTrue(scheduled().isEmpty())
        assertTrue(SentinelArmedAlarms.all(context).isEmpty())
    }

    @Test
    @Config(sdk = [31, 36])
    fun withoutTheExactAlarmPermissionItIsStillArmedInexactly() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("x", now + 3_600_000, null)
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        reboot(Intent.ACTION_BOOT_COMPLETED)
        val alarm = scheduled().single()
        assertNull(alarm.alarmClockInfo)
        assertEquals(now + 3_600_000, alarm.triggerAtMs)
    }

    /** Before the first unlock the app's files are closed: nothing can be read, so nothing is armed. */
    @Test
    @Config(sdk = [33])
    fun nothingIsArmedWhileTheFilesAreLocked() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("x", now + 3_600_000, null)
        val users = shadowOf(context.getSystemService(UserManager::class.java))
        users.setUserUnlocked(false)
        reboot(Intent.ACTION_LOCKED_BOOT_COMPLETED)
        assertTrue(scheduled().isEmpty())
        users.setUserUnlocked(true)
        assertNotNull(SentinelArmedAlarms.all(context)["x"])
    }
}
