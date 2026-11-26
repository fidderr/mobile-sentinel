package com.mobilesentinel

import android.app.Application
import android.app.Notification
import android.content.Context
import android.content.Intent
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowSystemClock
import java.io.File
import java.time.Duration

/**
 * The ring service fed with the plans Rust really serializes (`tests/fixtures/ring-plan-*.json`,
 * pinned by `tests/kotlin_contract.rs`): a renamed or retyped field breaks one of the two.
 */
@RunWith(RobolectricTestRunner::class)
class RustRingPlanTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val vibrator get() = shadowOf(SentinelCompat.vibrator(context)!!)

    @Before
    fun setUp() {
        ShadowSystemClock.advanceBy(Duration.ofHours(1))
        SentinelPrimitives.init(context)
    }

    private fun fixture(name: String) = File(System.getProperty("sentinel.fixtures")!!, name).readText()

    private fun ring(plan: String): Notification {
        val dir = File(SentinelStorage.filesDir(context), "sentinel/jobs")
        dir.mkdirs()
        File(dir, "alarm-1.json").writeText("{}")
        assertTrue(SentinelActiveJobs.mark(context, "alarm-1"))
        SentinelRing.start(context, "alarm-1", plan, false)
        val service = Robolectric.buildService(SentinelForegroundService::class.java).create()
        val intent = Intent(context, SentinelForegroundService::class.java)
            .setAction(SentinelRing.ACTION_RING)
            .putExtra(SentinelRing.EXTRA_ID, "alarm-1")
        service.withIntent(intent).startCommand(0, 1)
        return shadowOf(service.get()).lastForegroundNotification
    }

    @Test
    @Config(sdk = [26, 28, 31, 33, 36])
    fun theFullPlanRingsWithEverythingItSays() {
        val n = ring(fixture("ring-plan-full.json"))
        assertEquals("Wake up", n.extras.getString(Notification.EXTRA_TITLE))
        assertEquals("07:00", n.extras.getString(Notification.EXTRA_TEXT))
        assertEquals("alarms", n.channelId)
        assertEquals("com.example.RingActivity", shadowOf(n.fullScreenIntent).savedIntent.component?.className)
        assertTrue(vibrator.isVibrating)
        val plan = SentinelRing.list(context).single().plan
        assertEquals(1800L, plan.getLong("ring_limit_secs"))
        assertEquals(3, plan.getInt("backup_holds"))
    }

    /** A plan with no app screen: `activity_fqcn` is JSON null, which must not become the class "null". */
    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun aPlanWithoutAnAppScreenHasNoFullScreenIntent() {
        val n = ring(fixture("ring-plan-minimal.json"))
        assertNull("the notification points at a screen named 'null'", n.fullScreenIntent)
        assertNull(n.contentIntent)
        assertNotNull(n)
        assertFalse(shadowOf(context as Application).nextStartedService == null)
    }
}
