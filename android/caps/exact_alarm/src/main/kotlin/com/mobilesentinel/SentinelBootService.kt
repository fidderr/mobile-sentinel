package com.mobilesentinel

import android.app.Service
import android.content.Intent
import android.os.IBinder
import android.util.Log
import org.json.JSONObject
import java.io.File

/**
 * Invisible background service that runs in MAIN process on boot.
 *
 * Android clears every scheduled alarm on reboot. Rust cannot re-arm them
 * here: a process started only for this service has no Activity, and the
 * UI framework registers the Android context the native code depends on only
 * when the Activity is created. So the re-arm is done in Kotlin, from the
 * persisted alarm records (the contract Rust writes while alive): every
 * `scheduled` or `snoozed` record whose fire time is still ahead gets its
 * exact alarm back. Anything already past due is left for the app's own
 * startup to reschedule. Then the service stops itself. The user never sees
 * anything — no activity, no notification.
 *
 * Started by [SentinelBootReceiver] after BOOT_COMPLETED.
 */
class SentinelBootService : Service() {

    companion object {
        private const val TAG = "MobileSentinel.BootSvc"
        private const val CONTEXT_DIR = "sentinel/context"
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i(TAG, "Boot service started")
        try {
            rearmStoredAlarms()
        } catch (e: Throwable) {
            Log.w(TAG, "re-arming stored alarms failed: ${e.message}")
        }
        // Lets Rust run its own boot hook if the consumer registered one.
        try {
            SentinelBridge.onBootCompleted()
        } catch (e: Throwable) {
            Log.w(TAG, "onBootCompleted callback failed: ${e.message}")
        }
        stopSelf()
        return START_NOT_STICKY
    }

    private fun rearmStoredAlarms() {
        val files = File(filesDir, CONTEXT_DIR).listFiles { f -> f.extension == "json" } ?: return
        val now = System.currentTimeMillis()
        var armed = 0
        for (file in files) {
            try {
                val json = JSONObject(file.readText())
                val state = json.optJSONObject("state") ?: continue
                val kind = state.optString("state")
                if (kind != "scheduled" && kind != "snoozed") continue
                val fireAt = state.optLong("next_fire_unix_ms", 0L)
                val id = json.optString("id")
                if (id.isBlank() || fireAt <= now) continue
                if (SentinelExactAlarmPrimitives.scheduleExactAlarm(id, fireAt, null)) armed++
            } catch (e: Throwable) {
                Log.w(TAG, "skipping ${file.name}: ${e.message}")
            }
        }
        Log.i(TAG, "re-armed $armed alarm(s) from the stored records")
    }

    override fun onBind(intent: Intent?): IBinder? = null
}
