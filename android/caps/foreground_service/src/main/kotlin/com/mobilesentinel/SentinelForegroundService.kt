package com.mobilesentinel

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ComponentName
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.util.Log

/**
 * Minimal foreground service. Its ONLY job: keep MAIN alive.
 *
 * Android kills background processes aggressively. A foreground service
 * with a visible notification prevents this. This service has NO logic —
 * no audio, no kiosk, no watchdog, no state management. All of that
 * lives in Rust (MAIN process).
 *
 * Consumer starts this via `SentinelForegroundServicePrimitives.startForegroundService()`
 * when an alarm fires, and stops it via `stopForegroundService()` when the
 * alarm is dismissed. The `fullScreen` flag is the FINAL decision made by
 * Rust (the firing sink decides whether the full-screen banner is redundant);
 * the service does not decide policy.
 */
class SentinelForegroundService : Service() {

    companion object {
        private const val TAG = "MobileSentinel.Service"
        private const val NOTIFICATION_ID = 9001

        const val ACTION_FGS_START = "com.mobilesentinel.service.FGS_START"
        const val ACTION_FGS_STOP = "com.mobilesentinel.service.FGS_STOP"

        const val EXTRA_CHANNEL_ID = "channel_id"
        const val EXTRA_CHANNEL_NAME = "channel_name"
        const val EXTRA_TITLE = "title"
        const val EXTRA_BODY = "body"
        const val EXTRA_IMPORTANCE = "importance"
        const val EXTRA_BYPASS_DND = "bypass_dnd"
        const val EXTRA_ACTIVITY_FQCN = "activity_fqcn"
        const val EXTRA_FULL_SCREEN = "full_screen"
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        Log.i(TAG, "onCreate")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action

        when (action) {
            ACTION_FGS_STOP -> {
                Log.i(TAG, "FGS_STOP received — stopping")
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                    stopForeground(STOP_FOREGROUND_REMOVE)
                } else {
                    @Suppress("DEPRECATION")
                    stopForeground(true)
                }
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_FGS_START, null -> {
                val channelId = intent?.getStringExtra(EXTRA_CHANNEL_ID) ?: "sentinel_foreground"
                val channelName = intent?.getStringExtra(EXTRA_CHANNEL_NAME) ?: "Background Service"
                val title = intent?.getStringExtra(EXTRA_TITLE) ?: "Active"
                val body = intent?.getStringExtra(EXTRA_BODY) ?: "Running"
                val importance = intent?.getIntExtra(EXTRA_IMPORTANCE, NotificationManager.IMPORTANCE_MAX)
                    ?: NotificationManager.IMPORTANCE_MAX
                val bypassDnd = intent?.getBooleanExtra(EXTRA_BYPASS_DND, true) ?: true
                val activityFqcn = intent?.getStringExtra(EXTRA_ACTIVITY_FQCN)
                val fullScreen = intent?.getBooleanExtra(EXTRA_FULL_SCREEN, true) ?: true

                // `fullScreen` is the FINAL decision made by Rust (the firing
                // sink): Rust already checked whether the firing UI is in the
                // foreground and suppressed the redundant banner if so. The
                // service does not decide policy — it just applies it.
                ensureChannel(channelId, channelName, importance, bypassDnd)
                val notification = buildNotification(channelId, title, body, activityFqcn, fullScreen)
                startForeground(NOTIFICATION_ID, notification)
                Log.i(TAG, "FGS_START — foreground title='$title' fullScreen=$fullScreen")
                return START_STICKY
            }
            else -> {
                Log.w(TAG, "Unknown action: $action")
                return START_STICKY
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        Log.i(TAG, "onDestroy")
    }

    private fun ensureChannel(
        channelId: String,
        channelName: String,
        importance: Int,
        bypassDnd: Boolean,
    ) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val nm = getSystemService(NotificationManager::class.java) ?: return
            if (nm.getNotificationChannel(channelId) != null) return
            val channel = NotificationChannel(channelId, channelName, importance).apply {
                setBypassDnd(bypassDnd)
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
                setSound(null, null)
            }
            nm.createNotificationChannel(channel)
            Log.i(TAG, "Created channel: $channelId (importance=$importance, bypassDnd=$bypassDnd)")
        }
    }

    private fun buildNotification(
        channelId: String,
        title: String,
        body: String,
        activityFqcn: String?,
        useFullScreen: Boolean,
    ): Notification {
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, channelId)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }

        builder.setContentTitle(title)
            .setContentText(body)
            .setSmallIcon(android.R.drawable.ic_lock_idle_alarm)
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_CALL)
            .setVisibility(Notification.VISIBILITY_PUBLIC)

        // Tap notification → open the activity
        if (activityFqcn != null) {
            val tapIntent = Intent().apply {
                component = ComponentName(packageName, activityFqcn)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or
                        Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or
                        Intent.FLAG_ACTIVITY_SINGLE_TOP
            }
            val pi = PendingIntent.getActivity(
                this, 0, tapIntent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            builder.setContentIntent(pi)
            // Only attach the full-screen intent when requested — this is what
            // produces the on-screen banner. Rust already decided whether it
            // is needed (suppressed when the UI is already foreground).
            if (useFullScreen) {
                builder.setFullScreenIntent(pi, true)
            }
        }

        return builder.build()
    }
}
