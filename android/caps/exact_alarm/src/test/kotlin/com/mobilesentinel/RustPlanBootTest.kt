package com.mobilesentinel

import android.content.Intent
import com.mobilesentinel.ExactAlarmTestSupport.context
import com.mobilesentinel.ExactAlarmTestSupport.forgetAllOsAlarms
import com.mobilesentinel.ExactAlarmTestSupport.scheduled
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAlarmManager
import org.robolectric.shadows.ShadowSystemClock
import java.io.File
import java.time.Duration

/** The boot replay reads `late_ring_window_ms` out of the plan Rust really writes (tests/fixtures). */
@RunWith(RobolectricTestRunner::class)
class RustPlanBootTest {
    @Before
    fun setUp() {
        ShadowSystemClock.advanceBy(Duration.ofHours(1))
        SentinelPrimitives.init(context)
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
    }

    private fun fixture(name: String) = File(System.getProperty("sentinel.fixtures")!!, name).readText()

    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun theLateWindowOfTheFullPlanLetsAMissedAlarmRing() {
        val now = System.currentTimeMillis()
        SentinelExactAlarmPrimitives.scheduleExactAlarm("alarm-1", now - 120_000, fixture("ring-plan-full.json"))
        forgetAllOsAlarms()
        SentinelBootReceiver().onReceive(context, Intent(Intent.ACTION_BOOT_COMPLETED))
        assertEquals(1, scheduled().size)
        assertTrue(scheduled().single().let { (it.alarmClockInfo?.triggerTime ?: it.triggerAtMs) >= now })
    }

    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun theMinimalPlanAllowsNoLateness() {
        val now = System.currentTimeMillis()
        SentinelExactAlarmPrimitives.scheduleExactAlarm("alarm-1", now - 120_000, fixture("ring-plan-minimal.json"))
        forgetAllOsAlarms()
        SentinelBootReceiver().onReceive(context, Intent(Intent.ACTION_BOOT_COMPLETED))
        assertTrue(scheduled().isEmpty())
    }
}
