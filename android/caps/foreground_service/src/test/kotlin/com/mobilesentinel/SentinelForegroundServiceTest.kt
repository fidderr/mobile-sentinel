package com.mobilesentinel

import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.os.UserManager
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
import org.robolectric.android.controller.ServiceController
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAlarmManager
import java.io.File

/** The ring service: the plan in, a foreground notification and the outputs out. */
@RunWith(RobolectricTestRunner::class)
class SentinelForegroundServiceTest {
    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val app get() = context as Application
    private val vibrator get() = shadowOf(SentinelCompat.vibrator(context)!!)

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

    private fun command(
        controller: ServiceController<SentinelForegroundService>,
        action: String?,
        id: String? = null,
    ): ServiceController<SentinelForegroundService> {
        val intent = Intent(context, SentinelForegroundService::class.java)
        if (action != null) intent.action = action
        if (id != null) intent.putExtra(SentinelRing.EXTRA_ID, id)
        return controller.withIntent(intent).startCommand(0, 1)
    }

    private fun ringing(plan: String, id: String = "a1"): ServiceController<SentinelForegroundService> {
        createJob(id)
        SentinelRing.start(context, id, plan, false)
        return command(Robolectric.buildService(SentinelForegroundService::class.java).create(), SentinelRing.ACTION_RING, id)
    }

    private fun notification(controller: ServiceController<SentinelForegroundService>): Notification =
        shadowOf(controller.get()).lastForegroundNotification

    @Test
    @Config(sdk = [26, 28, 30, 31, 33, 34, 36])
    fun aRingGoesToTheForegroundWithItsOwnTexts() {
        val c = ringing("""{"title":"Wake up","body":"07:00","channel_id":"alarms","sound_uri":"silent://","vibrate":false}""")
        val n = notification(c)
        assertNotNull("startForeground was not called", n)
        assertEquals("Wake up", n.extras.getString(Notification.EXTRA_TITLE))
        assertEquals("07:00", n.extras.getString(Notification.EXTRA_TEXT))
        assertEquals("alarms", n.channelId)
        assertEquals(Notification.CATEGORY_ALARM, n.category)
        assertTrue(n.flags and Notification.FLAG_ONGOING_EVENT != 0)
        val channel = context.getSystemService(NotificationManager::class.java).getNotificationChannel("alarms")
        assertNotNull(channel)
        assertEquals(NotificationManager.IMPORTANCE_MAX, channel.importance)
    }

    @Test
    @Config(sdk = [26, 28, 30, 31, 33, 36])
    fun theVibrationStartsOnAnyVersion() {
        ringing("""{"sound_uri":"silent://","vibrate":true,"vibration_pattern":[0,300,100,300]}""")
        assertTrue(vibrator.isVibrating)
    }

    @Test
    @Config(sdk = [28, 33])
    fun aSilentPlanThatDoesNotVibrateStaysQuiet() {
        ringing("""{"sound_uri":"silent://","vibrate":false}""")
        assertFalse(vibrator.isVibrating)
    }

    @Test
    @Config(sdk = [28, 33])
    fun theFullScreenIntentPointsAtTheAppScreen() {
        val c = ringing("""{"sound_uri":"silent://","vibrate":false,"activity_fqcn":"com.example.RingActivity","firing_full_screen":true}""")
        val n = notification(c)
        assertNotNull(n.fullScreenIntent)
        assertEquals("com.example.RingActivity", shadowOf(n.fullScreenIntent).savedIntent.component?.className)
    }

    @Test
    @Config(sdk = [28, 33])
    fun withoutFullScreenThereIsNoFullScreenIntent() {
        val c = ringing("""{"sound_uri":"silent://","vibrate":false,"activity_fqcn":"com.example.RingActivity","firing_full_screen":false}""")
        assertNull(notification(c).fullScreenIntent)
    }

    /** Before the first unlock the app's screen cannot start: the unlock screen takes its place. */
    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun whileLockedTheUnlockScreenStandsInForTheApp() {
        shadowOf(context.getSystemService(UserManager::class.java)).setUserUnlocked(false)
        SentinelStorage.isDirectBoot(context)
        val c = ringing("""{"sound_uri":"silent://","vibrate":false,"activity_fqcn":"com.example.RingActivity"}""")
        val n = notification(c)
        assertEquals(
            "com.mobilesentinel.SentinelUnlockActivity",
            shadowOf(n.fullScreenIntent).savedIntent.component?.className,
        )
    }

    /** The alarm was dismissed before the service got to it: no ring, and the service ends. */
    @Test
    @Config(sdk = [28, 33])
    fun aRingWhoseJobIsNotActiveEndsTheService() {
        val c = command(Robolectric.buildService(SentinelForegroundService::class.java).create(), SentinelRing.ACTION_RING, "gone")
        assertTrue(shadowOf(c.get()).isStoppedBySelf)
        assertFalse(vibrator.isVibrating)
    }

    @Test
    @Config(sdk = [26, 28, 33, 36])
    fun stopEndsTheOutputsTheWatchdogAndTheService() {
        val c = ringing("""{"sound_uri":"silent://","vibrate":true}""")
        assertTrue(vibrator.isVibrating)
        command(c, SentinelRing.ACTION_STOP, "a1")
        assertFalse(vibrator.isVibrating)
        assertTrue(shadowOf(c.get()).isStoppedBySelf)
        assertTrue(shadowOf(context.getSystemService(Context.ALARM_SERVICE) as android.app.AlarmManager).scheduledAlarms.isEmpty())
    }

    /** Android restarts a killed sticky service with no intent: whatever still rings must come back. */
    @Test
    @Config(sdk = [28, 33])
    fun aRestartWithoutAnIntentBringsTheRingBack() {
        createJob("a1")
        SentinelRing.start(context, "a1", """{"sound_uri":"silent://","vibrate":true}""", false)
        val c = command(Robolectric.buildService(SentinelForegroundService::class.java).create(), null)
        assertTrue(vibrator.isVibrating)
        assertNotNull(notification(c))
    }

    private fun watchdogs() = shadowOf(context.getSystemService(Context.ALARM_SERVICE) as android.app.AlarmManager)
        .scheduledAlarms.filter { shadowOf(it.operation).savedIntent.getStringExtra("instance_id") == SentinelRing.WATCHDOG_ID }

    /** A kill in the first second of a ring must find a watchdog armed: the first tick is not the time to arm it. */
    @Test
    @Config(sdk = [28, 33])
    fun aRingHasAWatchdogFromTheMomentItStarts() {
        ringing("""{"sound_uri":"silent://"}""")
        assertEquals(1, watchdogs().size)
    }

    @Test
    @Config(sdk = [28, 33])
    fun aRingResumedAfterAKillHasAWatchdogFromTheMomentItStarts() {
        createJob("a1")
        SentinelRing.start(context, "a1", """{"sound_uri":"silent://"}""", false)
        val alarms = context.getSystemService(Context.ALARM_SERVICE) as android.app.AlarmManager
        for (a in shadowOf(alarms).scheduledAlarms.toList()) alarms.cancel(a.operation!!)
        assertTrue(watchdogs().isEmpty())
        command(Robolectric.buildService(SentinelForegroundService::class.java).create(), SentinelRing.ACTION_RESUME)
        assertEquals(1, watchdogs().size)
    }

    @Test
    @Config(sdk = [28, 33])
    fun pauseSilencesTheOutputsAndRingBringsThemBack() {
        var c = ringing("""{"sound_uri":"silent://","vibrate":true}""")
        command(c, SentinelRing.ACTION_PAUSE)
        assertFalse(vibrator.isVibrating)
        c = command(c, SentinelRing.ACTION_RING, "a1")
        assertTrue(vibrator.isVibrating)
    }
}
