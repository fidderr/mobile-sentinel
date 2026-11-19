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
    @Test
    @Config(sdk = [33])
    fun theWatchdogDoesNotActivateAJob() {
        writeJobFile("a1")
        deliver("a1" + SentinelAlarmReceiver.WATCHDOG_SUFFIX, null)
        assertFalse(SentinelActiveJobs.isActive(context, "a1"))
        assertNull(shadowOf(app).nextStartedService)
    }
}
