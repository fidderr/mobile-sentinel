package com.mobilesentinel

import android.app.AlarmManager
import com.mobilesentinel.ExactAlarmTestSupport.context
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

@RunWith(RobolectricTestRunner::class)
class SentinelExactAlarmPrimitivesTest {
    private val at = System.currentTimeMillis() + 3_600_000L

    @Before
    fun setUp() {
        // A phone that has been up for an hour: the files these tests write are newer than the boot.
        org.robolectric.shadows.ShadowSystemClock.advanceBy(java.time.Duration.ofHours(1))
        SentinelPrimitives.init(context)
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
    }

    @Test
    @Config(sdk = [26, 28, 29, 30, 31, 33, 34, 36])
    fun withPermissionTheAlarmIsAnAlarmClockWithItsPlan() {
        assertTrue(SentinelExactAlarmPrimitives.scheduleExactAlarm("a1", at, """{"title":"Wake"}"""))
        val alarm = scheduled().single()
        assertEquals(at, alarm.alarmClockInfo?.triggerTime)
        val intent = shadowOf(alarm.operation).savedIntent
        assertEquals(SentinelAlarmReceiver.ACTION, intent.action)
        assertEquals("a1", intent.getStringExtra(SentinelAlarmReceiver.EXTRA_INSTANCE_ID))
        assertEquals("""{"title":"Wake"}""", intent.getStringExtra(SentinelAlarmReceiver.EXTRA_METADATA_JSON))
        assertEquals(at, SentinelArmedAlarms.all(context).getValue("a1").fireAtMs)
    }

    /** From Android 12 a user can take the permission away: the alarm is still set, but not exact. */
    @Test
    @Config(sdk = [31, 33, 34, 36])
    fun withoutPermissionTheAlarmFallsBackToAnInexactOne() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        assertTrue(SentinelExactAlarmPrimitives.scheduleExactAlarm("a1", at, null))
        val alarm = scheduled().single()
        assertNull(alarm.alarmClockInfo)
        assertEquals(AlarmManager.RTC_WAKEUP, alarm.type)
        assertEquals(at, alarm.triggerAtMs)
        assertNotNull(SentinelArmedAlarms.all(context)["a1"])
    }

    /** Below Android 12 there is no such permission, and asking the OS would be a NoSuchMethodError. */
    @Test
    @Config(sdk = [26, 28, 29, 30])
    fun belowAndroid12TheAlarmIsAlwaysAnAlarmClock() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        assertTrue(SentinelExactAlarmPrimitives.scheduleExactAlarm("a1", at, null))
        assertNotNull(scheduled().single().alarmClockInfo)
    }

    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun settingTheSameIdAgainReplacesTheAlarm() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("a1", at, null)
        SentinelExactAlarmPrimitives.scheduleExactAlarm("a1", at + 60_000, null)
        assertEquals(at + 60_000, scheduled().single().alarmClockInfo?.triggerTime)
        assertEquals(at + 60_000, SentinelArmedAlarms.all(context).getValue("a1").fireAtMs)
    }

    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun cancelRemovesTheAlarmAndItsRecord() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("a1", at, null)
        SentinelExactAlarmPrimitives.scheduleExactAlarm("a2", at, null)
        assertTrue(SentinelExactAlarmPrimitives.cancelExactAlarm("a1"))
        assertEquals(1, scheduled().size)
        assertEquals(setOf("a2"), SentinelArmedAlarms.all(context).keys)
    }

    @Test
    @Config(sdk = [28, 33])
    fun cancellingAnAlarmThatIsNotSetIsFine() {
        assertTrue(SentinelExactAlarmPrimitives.cancelExactAlarm("nothing"))
        assertTrue(scheduled().isEmpty())
    }

    /** The ring's watchdog re-arms itself every few seconds and must not be replayed at boot. */
    @Test
    @Config(sdk = [28, 33])
    fun theWatchdogIsNotRecorded() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("ring#watchdog", at, null)
        assertEquals(1, scheduled().size)
        assertTrue(SentinelArmedAlarms.all(context).isEmpty())
    }

    @Test
    @Config(sdk = [28, 33])
    fun theArmedRecordKeepsIdsThatNeedEncoding() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("a/b c#1", at, """{"x":1}""")
        assertEquals("""{"x":1}""", SentinelArmedAlarms.all(context).getValue("a/b c#1").metadataJson)
    }

    @Test
    @Config(sdk = [28, 33])
    fun anUnreadableRecordIsDroppedNotFatal() {
        SentinelExactAlarmPrimitives.scheduleExactAlarm("good", at, null)
        val dir = java.io.File(SentinelStorage.filesDir(context), "sentinel/armed")
        java.io.File(dir, "bad.armed").writeText("not a number")
        assertEquals(setOf("good"), SentinelArmedAlarms.all(context).keys)
        assertTrue(!java.io.File(dir, "bad.armed").exists())
    }
}
