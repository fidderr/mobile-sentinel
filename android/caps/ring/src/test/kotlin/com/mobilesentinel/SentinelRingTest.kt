package com.mobilesentinel

import android.app.AlarmManager
import android.app.Application
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
import java.io.File

@RunWith(RobolectricTestRunner::class)
class SentinelRingTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val app get() = context as Application
    private val alarms get() = shadowOf(context.getSystemService(Context.ALARM_SERVICE) as AlarmManager)

    @Before
    fun setUp() {
        // A phone that has been up for an hour: the files these tests write are newer than the boot.
        org.robolectric.shadows.ShadowSystemClock.advanceBy(java.time.Duration.ofHours(1))
        SentinelPrimitives.init(context)
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
    }

    private fun createJob(id: String) {
        val dir = File(SentinelStorage.filesDir(context), "sentinel/jobs")
        dir.mkdirs()
        File(dir, "$id.json").writeText("{}")
        assertTrue(SentinelActiveJobs.mark(context, id))
    }

    @Test
    @Config(sdk = [26, 28, 30, 31, 33, 34, 36])
    fun startKeepsThePlanAndStartsTheServiceInTheForeground() {
        createJob("a1")
        assertTrue(SentinelRing.start(context, "a1", """{"title":"Wake"}""", uiVisible = true))
        val intent = shadowOf(app).nextStartedService
        assertNotNull(intent)
        assertEquals(SentinelRing.ACTION_RING, intent.action)
        assertEquals("a1", intent.getStringExtra(SentinelRing.EXTRA_ID))
        assertTrue(intent.getBooleanExtra(SentinelRing.EXTRA_UI_VISIBLE, false))
        assertEquals("com.mobilesentinel.SentinelForegroundService", intent.component?.className)
        assertEquals("Wake", SentinelRing.list(context).single().plan.getString("title"))
    }

    @Test
    @Config(sdk = [33])
    fun startRingGoesThroughTheJniEntryWithTheAppContext() {
        createJob("a1")
        assertTrue(SentinelRing.startRing("a1", "{}", false))
        assertEquals(SentinelRing.ACTION_RING, shadowOf(app).nextStartedService.action)
    }

    /** The marker is the truth: a plan whose job is no longer active is not ringing, and is cleaned up. */
    @Test
    @Config(sdk = [33])
    fun aPlanWithoutAnActiveJobIsNotRinging() {
        SentinelRing.start(context, "gone", """{"title":"x"}""", false)
        assertTrue(SentinelRing.list(context).isEmpty())
        val plans = File(SentinelStorage.filesDir(context), "sentinel/ringing").listFiles { f -> f.name.endsWith(".plan") }
        assertTrue(plans.isNullOrEmpty())
    }

    /** An alarm must ring even when its plan cannot be read. */
    @Test
    @Config(sdk = [33])
    fun anUnreadablePlanStillRingsWithDefaults() {
        createJob("a1")
        SentinelRing.start(context, "a1", "this is not json", false)
        val ringing = SentinelRing.list(context).single()
        assertEquals("a1", ringing.id)
        assertEquals(0, ringing.plan.length())
    }

    @Test
    @Config(sdk = [33])
    fun stopDoesNothingForARingThatIsNotRinging() {
        assertTrue(SentinelRing.stop(context, "nope"))
        assertNull(shadowOf(app).nextStartedService)
    }

    @Test
    @Config(sdk = [26, 33, 36])
    fun stopTellsTheServiceWhichRingToEnd() {
        createJob("a1")
        SentinelRing.start(context, "a1", "{}", false)
        shadowOf(app).clearStartedServices()
        assertTrue(SentinelRing.stop(context, "a1"))
        val intent = shadowOf(app).nextStartedService
        assertEquals(SentinelRing.ACTION_STOP, intent.action)
        assertEquals("a1", intent.getStringExtra(SentinelRing.EXTRA_ID))
    }

    @Test
    @Config(sdk = [33])
    fun resumeSavedOnlyWhenSomethingRings() {
        assertFalse(SentinelRing.resumeSaved(context))
        assertNull(shadowOf(app).nextStartedService)
        createJob("a1")
        SentinelRing.start(context, "a1", "{}", false)
        shadowOf(app).clearStartedServices()
        assertTrue(SentinelRing.resumeSaved(context))
        assertEquals(SentinelRing.ACTION_RESUME, shadowOf(app).nextStartedService.action)
    }

    @Test
    @Config(sdk = [33])
    fun pauseQuietAndLoudAreCommandsToTheService() {
        assertTrue(SentinelRing.pauseRing())
        assertEquals(SentinelRing.ACTION_PAUSE, shadowOf(app).nextStartedService.action)
        assertTrue(SentinelRing.quietForUnlock(context))
        assertEquals(SentinelRing.ACTION_QUIET, shadowOf(app).nextStartedService.action)
        assertTrue(SentinelRing.endQuiet(context))
        assertEquals(SentinelRing.ACTION_LOUD, shadowOf(app).nextStartedService.action)
    }

    @Test
    @Config(sdk = [31, 33, 36])
    fun theWatchdogIsAnExactAlarmWhenThePermissionIsOn() {
        SentinelRing.armWatchdog(context)
        val alarm = alarms.scheduledAlarms.single()
        assertTrue(alarm.isAllowWhileIdle)
        assertEquals(AlarmManager.RTC_WAKEUP, alarm.type)
        assertEquals(SentinelRing.WATCHDOG_ID, shadowOf(alarm.operation).savedIntent.getStringExtra("instance_id"))
    }

    @Test
    @Config(sdk = [31, 33, 36])
    fun theWatchdogFallsBackToAnInexactAlarmWithoutThePermission() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        SentinelRing.armWatchdog(context)
        val alarm = alarms.scheduledAlarms.single()
        assertFalse(alarm.isAllowWhileIdle)
    }

    @Test
    @Config(sdk = [26, 28, 30])
    fun theWatchdogNeedsNoPermissionBelowAndroid12() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        SentinelRing.armWatchdog(context)
        assertTrue(alarms.scheduledAlarms.single().isAllowWhileIdle)
    }

    @Test
    @Config(sdk = [33])
    fun armingTheWatchdogTwiceKeepsOneAndCancelRemovesIt() {
        SentinelRing.armWatchdog(context)
        SentinelRing.armWatchdog(context)
        assertEquals(1, alarms.scheduledAlarms.size)
        SentinelRing.cancelWatchdog(context)
        assertTrue(alarms.scheduledAlarms.isEmpty())
    }

    @Test
    @Config(sdk = [33])
    fun theUiFailedMarkRoundTrips() {
        assertFalse(SentinelRing.uiFailed(context))
        SentinelRing.markUiFailed(context)
        assertTrue(SentinelRing.uiFailed(context))
        SentinelRing.clearUiFailed(context)
        assertFalse(SentinelRing.uiFailed(context))
    }
}
