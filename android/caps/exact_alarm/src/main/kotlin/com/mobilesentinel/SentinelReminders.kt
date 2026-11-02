package com.mobilesentinel

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log

/**
 * A quiet notification a while before an alarm rings. Nothing here touches the ring:
 * it has its own OS alarm, may arrive some minutes off when the phone is dozing, and if
 * it never comes the alarm still goes off.
 *
 * Rust sets these (`AlarmKit::set_reminder`) whenever alarms change or wake the app.
 */
object SentinelReminders {
    private const val TAG = "MobileSentinel.Reminder"
    private const val CHANNEL = "sentinel_reminders"
    const val EXTRA_TITLE = "title"
    const val EXTRA_BODY = "body"
    const val EXTRA_CHANNEL_NAME = "channel_name"
    const val EXTRA_LASTS_MS = "lasts_ms"
    const val EXTRA_ID = "id"

    private fun pending(context: Context, id: String, intent: Intent, flags: Int): PendingIntent? =
        PendingIntent.getBroadcast(
            context,
            0,
            intent.setClass(context, SentinelReminderReceiver::class.java)
                // Tells the reminders of two alarms apart.
                .setData(Uri.parse("reminder:$id")),
            flags or PendingIntent.FLAG_IMMUTABLE,
        )

    @JvmStatic
    fun set(id: String, atMs: Long, title: String, body: String, channelName: String, lastsMs: Long): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val intent = Intent()
                .putExtra(EXTRA_ID, id)
                .putExtra(EXTRA_TITLE, title)
                .putExtra(EXTRA_BODY, body)
                .putExtra(EXTRA_CHANNEL_NAME, channelName)
                .putExtra(EXTRA_LASTS_MS, lastsMs)
            val operation = pending(context, id, intent, PendingIntent.FLAG_UPDATE_CURRENT) ?: return false
            val manager = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
            manager.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, atMs, operation)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setting a reminder failed: ${e.message}")
            false
        }
    }

    @JvmStatic
    fun clear(id: String): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            pending(context, id, Intent(), PendingIntent.FLAG_NO_CREATE)?.let { operation ->
                val manager = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
                manager.cancel(operation)
                operation.cancel()
            }
            val notifications = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            notifications.cancel(id.hashCode())
            true
        } catch (e: Throwable) {
            Log.w(TAG, "clearing a reminder failed: ${e.message}")
            false
        }
    }

    fun show(context: Context, intent: Intent) {
        val id = intent.getStringExtra(EXTRA_ID) ?: return
        val notifications = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        notifications.createNotificationChannel(
            NotificationChannel(
                CHANNEL,
                intent.getStringExtra(EXTRA_CHANNEL_NAME) ?: CHANNEL,
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
        val open = context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
            PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE)
        }
        val notification = Notification.Builder(context, CHANNEL)
            .setSmallIcon(context.applicationInfo.icon)
            .setContentTitle(intent.getStringExtra(EXTRA_TITLE))
            .setContentText(intent.getStringExtra(EXTRA_BODY))
            .setContentIntent(open)
            .setAutoCancel(true)
            // Gone by the time the alarm itself takes over.
            .setTimeoutAfter(intent.getLongExtra(EXTRA_LASTS_MS, 0).coerceAtLeast(1))
            .build()
        notifications.notify(id.hashCode(), notification)
    }
}

class SentinelReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        try {
            SentinelReminders.show(context, intent)
        } catch (e: Throwable) {
            Log.w("MobileSentinel.Reminder", "showing a reminder failed: ${e.message}")
        }
    }
}
