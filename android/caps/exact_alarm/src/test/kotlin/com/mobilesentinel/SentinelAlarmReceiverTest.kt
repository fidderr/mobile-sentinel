package com.mobilesentinel

import android.app.Application
import android.content.Intent
import android.os.Looper
import com.mobilesentinel.ExactAlarmTestSupport.context
import com.mobilesentinel.ExactAlarmTestSupport.scheduled
import com.mobilesentinel.ExactAlarmTestSupport.writeJobFile
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

/** An alarm goes off: its plan is handed to the ring service before anything of the app runs. */
@RunWith(RobolectricTestRunner::class)
class SentinelAlarmReceiverTest {
    private val plan = """{"title":"Wake up","sound_uri":"silent://"}"""
    private val app get() = context as Application

    @Before
    fun setUp() {
        // A phone that has been up for an hour: the files these tests write are newer than the boot.
        org.robolectric.shadows.ShadowSystemClock.advanceBy(java.time.Duration.ofHours(1))
        SentinelPrimitives.init(context)
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
    }

    private fun deliver(id: String, plan: String?) {
        val intent = Intent(context, SentinelAlarmReceiver::class.java)
            .setAction(SentinelAlarmReceiver.ACTION)
            .putExtra(SentinelAlarmReceiver.EXTRA_INSTANCE_ID, id)
        if (plan != null) intent.putExtra(SentinelAlarmReceiver.EXTRA_METADATA_JSON, plan)
        context.sendBroadcast(intent)
        shadowOf(Looper.getMainLooper()).idle()
    }

    @Test
    @Config(sdk = [26, 28, 30, 31, 33, 34, 36])
    fun theRingServiceStartsWithThePlanAndTheJobBecomesActive() {
        writeJobFile("a1")
        SentinelExactAlarmPrimitives.scheduleExactAlarm("a1", System.currentTimeMillis() + 1000, plan)
        deliver("a1", plan)

        val service = shadowOf(app).nextStartedService
        assertNotNull("the ring service was not started", service)
        assertEquals(SentinelRing.ACTION_RING, service.action)
        assertEquals("a1", service.getStringExtra(SentinelRing.EXTRA_ID))
        assertEquals("com.mobilesentinel.SentinelForegroundService", service.component?.className)
        assertTrue(SentinelActiveJobs.isActive(context, "a1"))
        assertEquals("Wake up", SentinelRing.list(context).single().plan.getString("title"))
        // It fired: it must not be replayed at the next boot.
        assertFalse(SentinelArmedAlarms.all(context).containsKey("a1"))
    }

    @Test
    @Config(sdk = [33])
    fun itArmsTheRingWatchdog() {
        writeJobFile("a1")
        deliver("a1", plan)
        val watchdog = scheduled().filter {
            shadowOf(it.operation).savedIntent.getStringExtra("instance_id") == SentinelRing.WATCHDOG_ID
        }
        assertEquals(1, watchdog.size)
    }

    /** An alarm whose job was deleted (cancelled meanwhile) must not ring. */
    @Test
    @Config(sdk = [33])
    fun withoutAJobFileNothingStarts() {
        deliver("ghost", plan)
        assertNull(shadowOf(app).nextStartedService)
        assertFalse(SentinelActiveJobs.isActive(context, "ghost"))
    }

    @Test
    @Config(sdk = [33])
    fun withoutAPlanTheJobIsActiveButNoServiceStarts() {
        writeJobFile("a1")
        deliver("a1", null)
        assertTrue(SentinelActiveJobs.isActive(context, "a1"))
        assertNull(shadowOf(app).nextStartedService)
    }

    /** The watchdog brings back a ring that is still going; it never starts a new one. */
    private fun watchdogs() = scheduled().filter {
        shadowOf(it.operation).savedIntent.getStringExtra("instance_id") == SentinelRing.WATCHDOG_ID
    }

    /** A ring brought back by the watchdog has a watchdog of its own at once, not a second later. */
    @Test
    @Config(sdk = [33])
    fun theWatchdogArmsTheNextOneWhileARingGoes() {
        writeJobFile("a1")
        SentinelActiveJobs.mark(context, "a1")
        SentinelRing.start(context, "a1", plan)
        assertEquals(SentinelRing.ACTION_RING, shadowOf(app).nextStartedService.action)
        assertTrue(watchdogs().isEmpty())
        deliver(SentinelRing.WATCHDOG_ID, null)
        assertEquals(1, watchdogs().size)
        assertEquals(SentinelRing.ACTION_RESUME, shadowOf(app).nextStartedService.action)
    }

    /** With nothing ringing the watchdog is a stray one: it brings nothing back and must not keep itself going. */
    @Test
    @Config(sdk = [33])
    fun theWatchdogArmsNothingWhenNothingRings() {
        deliver(SentinelRing.WATCHDOG_ID, null)
        assertTrue(watchdogs().isEmpty())
    }

    @Test
    @Config(sdk = [33])
    fun theWatchdogDoesNotActivateAJob() {
        writeJobFile("a1")
        deliver("a1" + SentinelAlarmReceiver.WATCHDOG_SUFFIX, null)
        assertFalse(SentinelActiveJobs.isActive(context, "a1"))
        assertNull(shadowOf(app).nextStartedService)
    }

    private fun dieFile() = java.io.File(SentinelStorage.filesDir(context), "sentinel/debug_die_after_watchdog_resume")

    private fun ringGoing() {
        writeJobFile("a1")
        SentinelActiveJobs.mark(context, "a1")
        SentinelRing.start(context, "a1", plan)
        shadowOf(app).nextStartedService
        dieFile().parentFile?.mkdirs()
        dieFile().writeText("")
    }

    /** The device test of the watchdog: a debug build ends its process once after a resume, and says so by taking the file. */
    @Test
    @Config(sdk = [33])
    fun aDebugBuildTakesTheDieFileOnTheWatchdogResume() {
        ringGoing()
        app.applicationInfo.flags = app.applicationInfo.flags or android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE
        org.robolectric.shadows.ShadowProcess.clearKilledProcesses()
        deliver(SentinelRing.WATCHDOG_ID, null)
        assertFalse(dieFile().exists())
        assertTrue(org.robolectric.shadows.ShadowProcess.wasKilled(android.os.Process.myPid()))
    }

    @Test
    @Config(sdk = [33])
    fun aReleaseBuildLeavesTheDieFileAlone() {
        ringGoing()
        app.applicationInfo.flags = app.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE.inv()
        org.robolectric.shadows.ShadowProcess.clearKilledProcesses()
        deliver(SentinelRing.WATCHDOG_ID, null)
        assertTrue(dieFile().exists())
        assertFalse(org.robolectric.shadows.ShadowProcess.wasKilled(android.os.Process.myPid()))
    }

    @Test
    @Config(sdk = [33])
    fun theDieFileIsNotTakenWhenNothingWasResumed() {
        dieFile().parentFile?.mkdirs()
        dieFile().writeText("")
        app.applicationInfo.flags = app.applicationInfo.flags or android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE
        deliver(SentinelRing.WATCHDOG_ID, null)
        assertTrue(dieFile().exists())
    }
}
