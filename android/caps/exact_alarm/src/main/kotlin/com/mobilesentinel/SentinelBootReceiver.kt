package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Generic boot receiver. On device boot Android clears every entry from
 * `AlarmManager`, so we start MAIN (which re-arms every persisted instance
 * via the consumer's startup) and the job guardian (in case an alarm was
 * firing when the device powered off).
 */
class SentinelBootReceiver : BroadcastReceiver() {

    companion object {
        private const val TAG = "MobileSentinel.Boot"
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED &&
            intent.action != "android.intent.action.QUICKBOOT_POWERON" &&
            intent.action != Intent.ACTION_LOCKED_BOOT_COMPLETED
        ) {
            return
        }

        Log.i(TAG, "Boot completed — starting boot service to re-arm alarms")
        SentinelPrimitives.init(context.applicationContext)

        // Start the boot service in MAIN process. This ensures the MAIN
        // process starts (loading Rust), the consumer's startup runs (re-arms
        // all alarms), then the service stops itself. No UI shown.
        try {
            val serviceIntent = Intent(context.applicationContext, SentinelBootService::class.java)
            context.applicationContext.startService(serviceIntent)
        } catch (e: Throwable) {
            Log.w(TAG, "Failed to start boot service: ${e.message}")
        }

        // Also start the job guardian in case there are active jobs
        // (e.g. alarm was firing when device powered off).
        SentinelJobGuardian.startGuarding(context.applicationContext)
    }
}
