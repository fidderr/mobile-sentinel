package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import org.json.JSONObject

/**
 * Generic boot receiver. On device boot Android clears every entry from
 * `AlarmManager`. This replays what the exact-alarm module armed
 * ([SentinelArmedAlarms]), which works even when Rust cannot run:
 * [SentinelRearmReceiver] then has the app recompute it all, if the host app lets it.
 * An alarm whose time passed while the phone was off still rings if its plan allows
 * for being that late.
 *
 * Runs in `:sentinel` because it starts the job guardian, which must not live in
 * MAIN (some OEMs, e.g. ColorOS, hold BOOT_COMPLETED back until the user opens the
 * app, so MAIN is often already up).
 */
class SentinelBootReceiver : BroadcastReceiver() {

    companion object {
        private const val TAG = "MobileSentinel.Boot"
        private const val LATE_RING_DELAY_MS = 1_000L
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED &&
            intent.action != "android.intent.action.QUICKBOOT_POWERON" &&
            intent.action != Intent.ACTION_LOCKED_BOOT_COMPLETED
        ) {
            return
        }

        Log.i(TAG, "Boot completed — re-arming alarms")
        val app = context.applicationContext
        SentinelPrimitives.init(app)

        // A reboot mid-ring left the volume up, with nothing left to put it back.
        SentinelRing.restoreVolumeIfIdle(app)

        try {
            replayArmedAlarms(context)
        } catch (e: Throwable) {
            Log.w(TAG, "re-arming alarms failed: ${e.message}")
        }

        // Also start the job guardian in case there are active jobs
        // (e.g. alarm was firing when device powered off).
        SentinelJobGuardian.startGuarding(app)
    }

    private fun replayArmedAlarms(context: Context) {
        val now = System.currentTimeMillis()
        var armed = 0
        for ((id, alarm) in SentinelArmedAlarms.all(context)) {
            val at = if (alarm.fireAtMs > now) {
                alarm.fireAtMs
            } else if (now - alarm.fireAtMs <= lateRingWindow(alarm)) {
                now + LATE_RING_DELAY_MS
            } else {
                // Too late to ring. Rust reports it as missed when it next runs.
                SentinelArmedAlarms.forget(context, id)
                continue
            }
            if (SentinelExactAlarmPrimitives.scheduleExactAlarm(id, at, alarm.metadataJson)) armed++
        }
        Log.i(TAG, "re-armed $armed alarm(s) from the armed record")
    }

    private fun lateRingWindow(alarm: SentinelArmedAlarms.Armed): Long {
        val plan = alarm.metadataJson ?: return 0
        return try {
            JSONObject(plan).optLong("late_ring_window_ms", 0)
        } catch (_: Throwable) {
            0
        }
    }
}
