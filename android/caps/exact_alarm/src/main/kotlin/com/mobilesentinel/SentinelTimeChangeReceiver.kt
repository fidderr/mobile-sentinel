package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Thin BroadcastReceiver for time / time-zone / date changes.
 *
 * On every TIME_CHANGED, TIMEZONE_CHANGED, DATE_CHANGED it starts the
 * SentinelJobGuardian, which brings MAIN up. MAIN's startup re-arms
 * Scheduled / Snoozed instances against the new clock.
 *
 * Registered statically in AndroidManifest with the same three
 * intent-filter actions.
 */
class SentinelTimeChangeReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val action = intent.action ?: return
        Log.i(TAG, "onReceive action=$action — starting MAIN to re-arm alarms")
        SentinelPrimitives.init(context.applicationContext)
        // Time/timezone changed — MAIN needs to re-arm all alarms with
        // the new clock. Start the job guardian which will start MAIN.
        SentinelJobGuardian.startGuarding(context.applicationContext)
    }

    companion object {
        private const val TAG = "MobileSentinel.TZ"
    }
}
