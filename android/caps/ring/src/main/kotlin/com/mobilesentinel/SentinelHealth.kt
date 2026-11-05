package com.mobilesentinel

import android.app.ActivityManager
import android.app.AlarmManager
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.SystemClock
import android.os.PowerManager
import android.provider.Settings
import android.util.Log
import org.json.JSONObject
import java.io.File

/**
 * What the phone allows, for an app that needs alarms to ring on time, and how to
 * get to where each is changed. Rust decides what counts as a problem and what to
 * say about it; this only reads and opens. Fields are read in `features::health`.
 */
object SentinelHealth {
    private const val TAG = "MobileSentinel.Health"
    private const val BOOT_SIGNAL_WAIT_MS = 5 * 60 * 1000L

    @JvmStatic
    fun snapshot(channelId: String): String {
        val c = SentinelPrimitives.getAppContext() ?: return "{}"
        val out = JSONObject()
        val read = { key: String, value: () -> Any ->
            try {
                out.put(key, value())
            } catch (e: Throwable) {
                Log.w(TAG, "reading $key failed: ${e.message}")
            }
        }
        val nm = c.getSystemService(NotificationManager::class.java)
        read("exact_alarms") {
            c.getSystemService(AlarmManager::class.java).canScheduleExactAlarms()
        }
        read("notifications") { nm.areNotificationsEnabled() }
        read("full_screen") {
            Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE || nm.canUseFullScreenIntent()
        }
        read("overlay") { Settings.canDrawOverlays(c) }
        read("battery_unrestricted") {
            c.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(c.packageName)
        }
        read("background_allowed") {
            !c.getSystemService(ActivityManager::class.java).isBackgroundRestricted
        }
        read("kept_when_unused") {
            c.packageManager.isAutoRevokeWhitelisted
        }
        read("channel_importance") {
            nm.getNotificationChannel(channelId)?.importance ?: -1
        }
        read("total_silence") {
            nm.currentInterruptionFilter == NotificationManager.INTERRUPTION_FILTER_NONE
        }
        read("boot_signal_missed") { bootSignalMissed(c) }
        return out.toString()
    }

    private fun bootSignalFile(c: Context) = File(SentinelStorage.filesDir(c), "sentinel/boot_signal")

    /** The phone told this app that it restarted. */
    @JvmStatic
    fun noteBootSignal(c: Context) {
        try {
            val file = bootSignalFile(c)
            file.parentFile?.mkdirs()
            file.writeText("")
        } catch (e: Throwable) {
            Log.w(TAG, "noting the boot signal failed: ${e.message}")
        }
    }

    /**
     * Some makers keep the boot signal from an app behind a switch of their own
     * ("auto-start"), which no app can read and often no app can open. So this goes by what
     * happened: the phone restarted with this app on it, and the signal never came.
     */
    private fun bootSignalMissed(c: Context): Boolean {
        val boot = SentinelBoot.bootTimeMs()
        // It can take a while after a restart.
        if (SystemClock.elapsedRealtime() < BOOT_SIGNAL_WAIT_MS) return false
        // Installed or updated since: there was nothing to tell it.
        if (c.packageManager.getPackageInfo(c.packageName, 0).lastUpdateTime >= boot) return false
        val file = bootSignalFile(c)
        return !file.exists() || file.lastModified() < boot
    }

    /** Open the system page where [check] is changed. [check] is a field of [snapshot]. */
    @JvmStatic
    fun openSettings(check: String, channelId: String): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        val pkg = Uri.parse("package:${c.packageName}")
        val appDetails = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, pkg)
        val intent = when (check) {
            "exact_alarms" -> Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, pkg)
            "notifications" -> Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                .putExtra(Settings.EXTRA_APP_PACKAGE, c.packageName)
            "full_screen" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                Intent(Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, pkg)
            } else {
                appDetails
            }
            "overlay" -> Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, pkg)
            "battery_unrestricted" -> Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
            "channel_importance" -> Intent(Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS)
                .putExtra(Settings.EXTRA_APP_PACKAGE, c.packageName)
                .putExtra(Settings.EXTRA_CHANNEL_ID, channelId)
            // Where Android has a page for it. Some makers send this to the app's own page,
            // where the switch is too.
            "kept_when_unused" -> Intent(Intent.ACTION_AUTO_REVOKE_PERMISSIONS, pkg)
            // Background use and the maker's auto-start switch live on or near the app's page.
            else -> appDetails
        }
        return try {
            c.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            true
        } catch (e: Throwable) {
            Log.w(TAG, "opening settings for $check failed: ${e.message}")
            try {
                c.startActivity(appDetails.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                true
            } catch (_: Throwable) {
                false
            }
        }
    }
}
