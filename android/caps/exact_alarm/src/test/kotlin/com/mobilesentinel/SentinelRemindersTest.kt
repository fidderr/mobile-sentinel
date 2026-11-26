package com.mobilesentinel

import android.app.NotificationManager
import android.content.Context
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

/** The notification before an alarm: silent on its own channel, or with the notification sound on another. */
@RunWith(RobolectricTestRunner::class)
class SentinelRemindersTest {
    private val manager get() = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    @Before
    fun setUp() {
        SentinelPrimitives.init(context)
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
    }

    private fun remind(id: String, sound: Boolean, body: String = "go to bed") {
        val now = System.currentTimeMillis()
        assertTrue(
            SentinelReminders.set(id, now + 1_000, "7:00", body, "Reminders", 8 * 3_600_000L, "Skip", "Skipped", "Undo", sound),
        )
        val intent = shadowOf(scheduled().single { shadowOf(it.operation).savedIntent.getStringExtra(SentinelReminders.EXTRA_ID) == id }.operation).savedIntent
        SentinelReminderReceiver().onReceive(context, intent)
    }

    @Test
    @Config(sdk = [26, 28, 31, 34, 36])
    fun aSilentReminderUsesTheQuietChannel() {
        remind("quiet", sound = false)
        val shown = shadowOf(manager).allNotifications.single()
        assertEquals("sentinel_reminders_popup", shown.channelId)
        assertNull(manager.getNotificationChannel("sentinel_reminders_popup").sound)
        assertEquals("go to bed", shown.extras.getString("android.text"))
    }

    @Test
    @Config(sdk = [26, 28, 31, 34, 36])
    fun aReminderWithSoundUsesItsOwnChannelWithTheNotificationSound() {
        remind("loud", sound = true)
        val shown = shadowOf(manager).allNotifications.single()
        assertEquals(SentinelReminders.SOUND_CHANNEL, shown.channelId)
        val channel = manager.getNotificationChannel(SentinelReminders.SOUND_CHANNEL)
        assertNotNull(channel.sound)
        assertEquals(NotificationManager.IMPORTANCE_HIGH, channel.importance)
        assertEquals("Reminders", channel.name.toString())
    }

    @Test
    @Config(sdk = [34])
    fun theTwoChannelsDoNotShareASound() {
        remind("a", sound = false)
        remind("b", sound = true)
        assertNull(manager.getNotificationChannel("sentinel_reminders_popup").sound)
        assertNotNull(manager.getNotificationChannel(SentinelReminders.SOUND_CHANNEL).sound)
    }

    @Test
    @Config(sdk = [34])
    fun aReminderComesAWholeDayAhead() {
        val now = System.currentTimeMillis()
        val at = now + 24 * 3_600_000L
        assertTrue(SentinelReminders.set("day", at, "t", "b", "n", 24 * 3_600_000L, "", "", "", false))
        assertEquals(at, scheduled().single().triggerAtMs)
    }
}
