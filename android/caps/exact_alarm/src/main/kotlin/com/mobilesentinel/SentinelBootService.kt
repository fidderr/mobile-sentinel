package com.mobilesentinel

import android.app.Service
import android.content.Intent
import android.os.IBinder
import android.util.Log

/**
 * Invisible background service that runs in MAIN process on boot.
 *
 * Android clears every scheduled alarm on reboot. Rust cannot re-arm them
 * here: a process started only for this service has no Activity, and the UI
 * framework registers the Android context the native code depends on only
 * when the Activity is created. So the OS-level arming is replayed in
 * Kotlin from [SentinelArmedAlarms], the record of what this module armed
 * (it never reads Rust's own files). Alarms whose time
 * already passed are dropped; the app's own startup reschedules those. Then
 * the service stops itself. The user never sees anything — no activity, no
 * notification.
 *
 * Started by [SentinelBootReceiver] after BOOT_COMPLETED.
 */
class SentinelBootService : Service() {

    companion object {
        private const val TAG = "MobileSentinel.BootSvc"
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i(TAG, "Boot service started")
        try {
            replayArmedAlarms()
        } catch (e: Throwable) {
            Log.w(TAG, "re-arming alarms failed: ${e.message}")
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

    private fun replayArmedAlarms() {
        val now = System.currentTimeMillis()
        var armed = 0
        for ((id, alarm) in SentinelArmedAlarms.all(this)) {
            if (alarm.fireAtMs <= now) {
                SentinelArmedAlarms.forget(this, id)
            } else if (SentinelExactAlarmPrimitives.scheduleExactAlarm(id, alarm.fireAtMs, alarm.metadataJson)) {
                armed++
            }
        }
        Log.i(TAG, "re-armed $armed alarm(s) from the armed record")
    }

    override fun onBind(intent: Intent?): IBinder? = null
}
