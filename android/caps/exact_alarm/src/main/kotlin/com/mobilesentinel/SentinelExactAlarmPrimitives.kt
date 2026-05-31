package com.mobilesentinel

import android.app.AlarmManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log

/**
 * Exact-alarm scheduling primitives. Rust (the firing sink, gated by the
 * `exact-alarm` Cargo feature) calls these to arm/cancel an OS exact alarm.
 * Thin executors around `AlarmManager` — Rust decides the time and id.
 */
object SentinelExactAlarmPrimitives {
    private const val TAG = "MobileSentinel.ExactAlarm"

    @JvmStatic
    fun scheduleExactAlarm(id: String, targetTimeMs: Long, metadataJson: String?): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val am = c.getSystemService(Context.ALARM_SERVICE) as AlarmManager
            val intent = Intent(c, SentinelAlarmReceiver::class.java).apply {
                action = SentinelAlarmReceiver.ACTION
                putExtra(SentinelAlarmReceiver.EXTRA_INSTANCE_ID, id)
                if (!metadataJson.isNullOrBlank()) {
                    putExtra(SentinelAlarmReceiver.EXTRA_METADATA_JSON, metadataJson)
                }
            }
            val pi = android.app.PendingIntent.getBroadcast(
                c,
                id.hashCode(),
                intent,
                android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_IMMUTABLE,
            )
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && !am.canScheduleExactAlarms()) {
                Log.w(TAG, "scheduleExactAlarm: missing SCHEDULE_EXACT_ALARM permission, falling back to inexact")
                am.set(AlarmManager.RTC_WAKEUP, targetTimeMs, pi)
            } else {
                am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, targetTimeMs, pi)
            }
            true
        } catch (e: Throwable) {
            Log.e(TAG, "scheduleExactAlarm($id): ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun cancelExactAlarm(id: String): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val am = c.getSystemService(Context.ALARM_SERVICE) as AlarmManager
            val intent = Intent(c, SentinelAlarmReceiver::class.java).apply {
                action = SentinelAlarmReceiver.ACTION
            }
            val pi = android.app.PendingIntent.getBroadcast(
                c,
                id.hashCode(),
                intent,
                android.app.PendingIntent.FLAG_NO_CREATE or android.app.PendingIntent.FLAG_IMMUTABLE,
            )
            if (pi != null) {
                am.cancel(pi)
                pi.cancel()
            }
            true
        } catch (e: Throwable) {
            Log.e(TAG, "cancelExactAlarm($id): ${e.message}", e)
            false
        }
    }
}
