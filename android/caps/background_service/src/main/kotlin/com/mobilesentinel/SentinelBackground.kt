package com.mobilesentinel

import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat

/**
 * Everything the `background_service` capability does that Rust calls into
 * (via JNI, see `crate::features::background_service`), plus the two native
 * hooks the host app implements:
 *
 *  - [nativeStart]: the service calls it (on a Java thread) to bring the app's
 *    core up when the process was started just for the service.
 *  - [nativeOnBack]: the Back button was pressed; the app decides what that
 *    means (go back a screen, or move to the background).
 *
 * Either missing is fine (UnsatisfiedLinkError is caught): the capability then
 * only keeps the process alive.
 */
object SentinelBackground {
    private const val TAG = "MobileSentinel.Bg"
    private const val PREFS = "sentinel_background"
    const val SERVICE_NOTIFICATION_ID = 7001
    private const val CH_SERVICE = "sentinel_background"
    private const val CH_MESSAGES = "sentinel_messages"
    private const val CH_CALLS = "sentinel_calls"
    private const val CH_REQUESTS = "sentinel_requests"

    /** What the UI should open, set by a notification tap, read once by the app. */
    @Volatile
    private var pendingOpen: String? = null

    @Volatile
    private var nativeStarted = false

    private fun prefs(c: Context): SharedPreferences =
        c.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    // ---- native hooks (implemented by the host app) ----

    @JvmStatic
    private external fun nativeStart(dataDir: String)

    @JvmStatic
    private external fun nativeOnBack()

    /** Brings the app's core up in this process (once). Safe to call from any Java thread. */
    @JvmStatic
    fun startNative(c: Context) {
        if (nativeStarted) return
        synchronized(this) {
            if (nativeStarted) return
            try {
                // Touching the bridge loads the app's native library.
                SentinelBridge.nativeLibraryName
                nativeStart(c.filesDir.absolutePath)
                nativeStarted = true
            } catch (e: UnsatisfiedLinkError) {
                Log.i(TAG, "host app has no nativeStart: the service only keeps the process alive")
                nativeStarted = true
            } catch (e: Throwable) {
                Log.e(TAG, "nativeStart failed: ${e.message}", e)
            }
        }
    }

    @JvmStatic
    fun onBack(activity: Activity): Boolean {
        return try {
            nativeOnBack()
            true
        } catch (e: UnsatisfiedLinkError) {
            false
        }
    }

    // ---- settings ----

    @JvmStatic
    fun isEnabled(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return prefs(c).getBoolean("enabled", false)
    }

    private fun setEnabledFlag(c: Context, on: Boolean) {
        prefs(c).edit().putBoolean("enabled", on).apply()
    }

    /** Texts for the service notification and the channel names (already translated). */
    @JvmStatic
    fun setTexts(
        serviceTitle: String,
        serviceText: String,
        messages: String,
        calls: String,
        requests: String,
    ) {
        val c = SentinelPrimitives.getAppContext() ?: return
        prefs(c).edit()
            .putString("service_title", serviceTitle)
            .putString("service_text", serviceText)
            .putString("ch_messages", messages)
            .putString("ch_calls", calls)
            .putString("ch_requests", requests)
            .apply()
    }

    // ---- the service ----

    /** Starts the service (and remembers it should run, also after a reboot). */
    @JvmStatic
    fun start(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        setEnabledFlag(c, true)
        return startService(c)
    }

    @JvmStatic
    fun startService(c: Context): Boolean {
        return try {
            ContextCompat.startForegroundService(c, Intent(c, SentinelBackgroundService::class.java))
            true
        } catch (e: Throwable) {
            Log.e(TAG, "could not start the service: ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun stop() {
        val c = SentinelPrimitives.getAppContext() ?: return
        setEnabledFlag(c, false)
        c.stopService(Intent(c, SentinelBackgroundService::class.java))
    }

    private fun ensureChannel(c: Context, id: String, name: String, importance: Int, configure: (NotificationChannel) -> Unit = {}) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val ch = NotificationChannel(id, name, importance)
        configure(ch)
        nm.createNotificationChannel(ch)
    }

    private fun launchIntent(c: Context): Intent? =
        c.packageManager.getLaunchIntentForPackage(c.packageName)

    /** The always-there notification of the service: as quiet as Android allows. */
    @JvmStatic
    fun serviceNotification(c: Context): Notification {
        val p = prefs(c)
        val label = c.applicationInfo.loadLabel(c.packageManager).toString()
        ensureChannel(c, CH_SERVICE, p.getString("service_title", label) ?: label, NotificationManager.IMPORTANCE_MIN) {
            it.setShowBadge(false)
            it.lockscreenVisibility = Notification.VISIBILITY_SECRET
        }
        val open = launchIntent(c)?.let {
            PendingIntent.getActivity(c, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        return NotificationCompat.Builder(c, CH_SERVICE)
            .setSmallIcon(com.mobilesentinel.backgroundservice.R.drawable.sentinel_ic_stat)
            .setContentTitle(p.getString("service_title", label))
            .setContentText(p.getString("service_text", ""))
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setVisibility(NotificationCompat.VISIBILITY_SECRET)
            .setOngoing(true)
            .setShowWhen(false)
            .setContentIntent(open)
            .build()
    }

    // ---- notifications ----

    /**
     * Posts (or replaces) a notification. [kind]: 0 message, 1 incoming call, 2 request.
     * [open] is handed back to the app by [takeOpenRequest] when the user taps it.
     */
    @JvmStatic
    fun post(tag: String, title: String, body: String, open: String, kind: Int): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val p = prefs(c)
            val (channel, name, importance) = when (kind) {
                1 -> Triple(CH_CALLS, p.getString("ch_calls", "Calls") ?: "Calls", NotificationManager.IMPORTANCE_HIGH)
                2 -> Triple(CH_REQUESTS, p.getString("ch_requests", "Requests") ?: "Requests", NotificationManager.IMPORTANCE_DEFAULT)
                else -> Triple(CH_MESSAGES, p.getString("ch_messages", "Messages") ?: "Messages", NotificationManager.IMPORTANCE_HIGH)
            }
            ensureChannel(c, channel, name, importance) {
                if (kind == 1) {
                    it.setSound(
                        android.media.RingtoneManager.getDefaultUri(android.media.RingtoneManager.TYPE_RINGTONE),
                        android.media.AudioAttributes.Builder()
                            .setUsage(android.media.AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
                            .build(),
                    )
                    it.enableVibration(true)
                }
            }
            val tap = Intent(c, SentinelOpenActivity::class.java).apply {
                putExtra("open", open)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            }
            val pi = PendingIntent.getActivity(
                c, tag.hashCode(), tap,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            val b = NotificationCompat.Builder(c, channel)
                .setSmallIcon(com.mobilesentinel.backgroundservice.R.drawable.sentinel_ic_stat)
                .setContentTitle(title)
                .setContentText(body.lineSequence().lastOrNull() ?: body)
                .setStyle(NotificationCompat.BigTextStyle().bigText(body))
                .setContentIntent(pi)
                .setAutoCancel(kind != 1)
                .setOnlyAlertOnce(false)
            if (kind == 1) {
                b.setCategory(NotificationCompat.CATEGORY_CALL)
                    .setPriority(NotificationCompat.PRIORITY_MAX)
                    .setOngoing(true)
                    .setTimeoutAfter(45_000)
                    .setFullScreenIntent(pi, true)
            } else {
                b.setCategory(NotificationCompat.CATEGORY_MESSAGE)
                    .setPriority(NotificationCompat.PRIORITY_HIGH)
            }
            val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            nm.notify(tag, 1, b.build())
            true
        } catch (e: Throwable) {
            Log.e(TAG, "post: ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun cancel(tag: String) {
        val c = SentinelPrimitives.getAppContext() ?: return
        (c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager).cancel(tag, 1)
    }

    /** Removes every notification we posted (not the service's own). */
    @JvmStatic
    fun cancelAll() {
        val c = SentinelPrimitives.getAppContext() ?: return
        val nm = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        try {
            nm.activeNotifications.filter { it.tag != null }.forEach { nm.cancel(it.tag, it.id) }
        } catch (e: Throwable) {
            Log.w(TAG, "cancelAll: ${e.message}")
        }
    }

    // ---- opening from a notification ----

    @JvmStatic
    fun setOpenRequest(what: String?) {
        pendingOpen = what?.takeIf { it.isNotEmpty() }
    }

    /** What a tapped notification asked the app to open, once. */
    @JvmStatic
    fun takeOpenRequest(): String? {
        val v = pendingOpen
        pendingOpen = null
        return v
    }

    // ---- the activity ----

    /** True while one of the app's activities is in front. */
    @JvmStatic
    fun isForeground(): Boolean = SentinelActivityTracker.currentResumedActivity != null

    /** Sends the app to the background (like Home), keeping its activity alive. */
    @JvmStatic
    fun moveToBack(): Boolean {
        val a = SentinelActivityTracker.currentResumedActivity ?: return false
        a.runOnUiThread { a.moveTaskToBack(true) }
        return true
    }
}
