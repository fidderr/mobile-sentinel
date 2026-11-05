package com.mobilesentinel

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
import android.util.Log
import java.io.File

/**
 * A silent notification a while before an alarm rings. Nothing here touches the ring:
 * it has its own OS alarm, and if it never comes the alarm still goes off.
 *
 * It can carry a button. A tap leaves a mark for Rust and starts it the way a boot does, so it
 * works with the app closed, and offers to take it back until the ring's time. Taking it back
 * brings the first notification back, button and all. If any of that fails nothing has changed,
 * and the alarm rings.
 *
 * Rust sets these (`AlarmKit::set_reminder`) whenever alarms change or wake the app.
 */
object SentinelReminders {
    private const val TAG = "MobileSentinel.Reminder"

    // Android lets nobody raise the importance of a channel that exists, and this one has to
    // pop up, so it is a new one. It makes no sound and does not vibrate: it only shows.
    private const val CHANNEL = "sentinel_reminders_popup"
    private const val OLD_CHANNEL = "sentinel_reminders"

    const val EXTRA_TITLE = "title"
    const val EXTRA_BODY = "body"
    const val EXTRA_CHANNEL_NAME = "channel_name"
    const val EXTRA_RING_AT_MS = "ring_at_ms"
    const val EXTRA_ID = "id"
    const val EXTRA_ACTION_LABEL = "action_label"
    const val EXTRA_DONE_TITLE = "done_title"
    const val EXTRA_UNDO_LABEL = "undo_label"

    const val ACTION_ACT = "com.mobilesentinel.REMINDER_ACT"
    const val ACTION_UNDO = "com.mobilesentinel.REMINDER_UNDO"
    private const val ACTION_WAKE = "com.mobilesentinel.REMINDER_TAPPED"

    /** Read by Rust, in `recipes/alarm_kit`: a file with the kind of tap and the alarm's id. */
    private const val TAPS_DIR = "reminder_taps"

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
    fun set(
        id: String,
        atMs: Long,
        title: String,
        body: String,
        channelName: String,
        lastsMs: Long,
        actionLabel: String,
        doneTitle: String,
        undoLabel: String,
    ): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val intent = Intent()
                .putExtra(EXTRA_ID, id)
                .putExtra(EXTRA_TITLE, title)
                .putExtra(EXTRA_BODY, body)
                .putExtra(EXTRA_CHANNEL_NAME, channelName)
                .putExtra(EXTRA_RING_AT_MS, atMs + lastsMs)
                .putExtra(EXTRA_ACTION_LABEL, actionLabel)
                .putExtra(EXTRA_DONE_TITLE, doneTitle)
                .putExtra(EXTRA_UNDO_LABEL, undoLabel)
            val operation = pending(context, id, intent, PendingIntent.FLAG_UPDATE_CURRENT) ?: return false
            val manager = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
            // To the minute where the app may. Left to itself Android puts this off by 75% of the
            // time until it is due, up to an hour: set the night before, that is past the ring it
            // is about, and it is then dropped.
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S || manager.canScheduleExactAlarms()) {
                manager.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, atMs, operation)
            } else {
                manager.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, atMs, operation)
            }
            // Still to come, so what shows now is about a ring that moved.
            val notifications = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            notifications.cancel(id.hashCode())
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

    private fun button(context: Context, kind: String, intent: Intent, label: String): Notification.Action {
        val id = intent.getStringExtra(EXTRA_ID).orEmpty()
        val tap = Intent(intent)
            .setAction(kind)
            .setClass(context, SentinelReminderReceiver::class.java)
            // Apart from the reminder's own alarm, and from the other kind of tap.
            .setData(Uri.parse("reminder-tap:$kind:$id"))
        val operation = PendingIntent.getBroadcast(
            context,
            0,
            tap,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Action.Builder(
            Icon.createWithResource(context, android.R.drawable.ic_lock_idle_alarm),
            label,
            operation,
        ).build()
    }

    private fun builder(context: Context, intent: Intent, showsForMs: Long): Notification.Builder {
        val notifications = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        notifications.deleteNotificationChannel(OLD_CHANNEL)
        notifications.createNotificationChannel(
            NotificationChannel(
                CHANNEL,
                intent.getStringExtra(EXTRA_CHANNEL_NAME) ?: CHANNEL,
                NotificationManager.IMPORTANCE_HIGH,
            ).apply {
                setSound(null, null)
                enableVibration(false)
                enableLights(false)
            },
        )
        val open = context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
            PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE)
        }
        return Notification.Builder(context, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_lock_idle_alarm)
            .setCategory(Notification.CATEGORY_REMINDER)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setContentIntent(open)
            .setAutoCancel(true)
            // Gone by the time the alarm itself takes over.
            .setTimeoutAfter(showsForMs)
    }

    private fun doneId(id: String) = "$id:done".hashCode()

    fun show(context: Context, intent: Intent) {
        val id = intent.getStringExtra(EXTRA_ID) ?: return
        val left = intent.getLongExtra(EXTRA_RING_AT_MS, 0) - System.currentTimeMillis()
        // A dozing phone can deliver this late, even after the ring.
        if (left <= 0) return
        val notification = builder(context, intent, left)
            .setContentTitle(intent.getStringExtra(EXTRA_TITLE))
            .setContentText(intent.getStringExtra(EXTRA_BODY))
        val label = intent.getStringExtra(EXTRA_ACTION_LABEL).orEmpty()
        if (label.isNotEmpty()) notification.addAction(button(context, ACTION_ACT, intent, label))
        val notifications = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        notifications.notify(id.hashCode(), notification.build())
    }

    fun tapped(context: Context, intent: Intent) {
        val id = intent.getStringExtra(EXTRA_ID) ?: return
        val notifications = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val left = intent.getLongExtra(EXTRA_RING_AT_MS, 0) - System.currentTimeMillis()
        val act = intent.action == ACTION_ACT
        notifications.cancel(id.hashCode())
        notifications.cancel(doneId(id))
        // Too late: it is ringing, and a ring is not ended from here.
        if (left <= 0) return
        File(SentinelStorage.filesDir(context), TAPS_DIR).let { dir ->
            dir.mkdirs()
            val name = id.hashCode().toString()
            val draft = File(dir, "$name.tmp")
            draft.writeText("${if (act) "act" else "undo"}\n$id")
            draft.renameTo(File(dir, name))
        }
        if (act) {
            val notification = builder(context, intent, left)
                .setContentTitle(intent.getStringExtra(EXTRA_DONE_TITLE))
                .setContentText(intent.getStringExtra(EXTRA_BODY))
            val undo = intent.getStringExtra(EXTRA_UNDO_LABEL).orEmpty()
            if (undo.isNotEmpty()) notification.addAction(button(context, ACTION_UNDO, intent, undo))
            notifications.notify(doneId(id), notification.build())
        } else {
            show(context, intent)
        }
        context.sendBroadcast(
            Intent(context, SentinelRearmReceiver::class.java)
                .setAction(ACTION_WAKE)
                .addFlags(Intent.FLAG_RECEIVER_FOREGROUND),
        )
    }
}

class SentinelReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        try {
            when (intent.action) {
                SentinelReminders.ACTION_ACT, SentinelReminders.ACTION_UNDO ->
                    SentinelReminders.tapped(context, intent)
                else -> SentinelReminders.show(context, intent)
            }
        } catch (e: Throwable) {
            Log.w("MobileSentinel.Reminder", "a reminder failed: ${e.message}")
        }
    }
}
