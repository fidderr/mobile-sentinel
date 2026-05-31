package com.mobilesentinel

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat

/**
 * Notifications capability — post / update / cancel general notifications.
 *
 * Lives in the `:sentinel-notifications` module, compiled only when the
 * `notifications` Cargo feature is enabled. Distinct from the firing
 * foreground-service notification, which the alarm runtime manages.
 *
 * Rust calls INTO these via JNI (`crate::features::notifications`). The app
 * context comes from [SentinelPrimitives.getAppContext].
 */
object SentinelNotificationPrimitives {
    private const val TAG = "MobileSentinel.Notif"

    @JvmStatic
    fun postNotification(
        id: String,
        channelId: String,
        title: String,
        body: String,
        importance: Int,
        fullScreenIntent: Boolean,
    ): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        ensureNotificationChannel(c, channelId, channelId, importance)
        return try {
            val builder = NotificationCompat.Builder(c, channelId)
                .setSmallIcon(android.R.drawable.ic_dialog_info)
                .setContentTitle(title)
                .setContentText(body)
                .setPriority(NotificationCompat.PRIORITY_HIGH)
            if (fullScreenIntent) {
                val launchIntent = c.packageManager.getLaunchIntentForPackage(c.packageName)
                if (launchIntent != null) {
                    val pi = PendingIntent.getActivity(
                        c,
                        id.hashCode(),
                        launchIntent,
                        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                    )
                    builder.setFullScreenIntent(pi, true)
                }
            }
            val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            nm.notify(id.hashCode(), builder.build())
            true
        } catch (e: Throwable) {
            Log.e(TAG, "postNotification: ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun cancelNotification(id: String): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            nm.cancel(id.hashCode())
            true
        } catch (e: Throwable) {
            Log.e(TAG, "cancelNotification: ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun updateNotification(id: String, title: String, body: String): Boolean {
        // Re-post under the same id with new content.
        return postNotification(
            id,
            "default",
            title,
            body,
            NotificationManager.IMPORTANCE_DEFAULT,
            false,
        )
    }

    private fun ensureNotificationChannel(
        c: Context,
        channelId: String,
        channelName: String,
        importance: Int,
    ) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (nm.getNotificationChannel(channelId) != null) return
        nm.createNotificationChannel(NotificationChannel(channelId, channelName, importance))
    }
}
