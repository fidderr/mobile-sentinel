package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Runs the app's core with no UI, in MAIN: when alarms may have to be armed again
 * (boot, an app update, the clock or time zone changing, exact alarms being allowed
 * again), and when an alarm goes off while MAIN is down, so its state machine runs
 * and the next occurrence gets armed whether or not the screen can come up.
 *
 * Rust decides what to do: this only starts it, through the hook the host app
 * implements. Without the hook alarms are armed when the app is next opened, and
 * after a boot by [SentinelBootReceiver]'s replay.
 */
class SentinelRearmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Log.i(TAG, "onReceive: ${intent.action}")
        SentinelPrimitives.init(context.applicationContext)
        if (!SentinelBoot.filesAvailable(context)) return
        val pending = goAsync()
        Thread({
            try {
                // Touching the bridge loads the app's native library.
                SentinelBridge.nativeLibraryName
                SentinelRearm.nativeRearm()
                if (intent.action == Intent.ACTION_BOOT_COMPLETED) SentinelBridge.onBootCompleted()
            } catch (e: UnsatisfiedLinkError) {
                Log.i(TAG, "host app has no nativeRearm: alarms are armed when it next opens")
            } catch (e: Throwable) {
                Log.e(TAG, "re-arming failed: ${e.message}", e)
            } finally {
                pending.finish()
            }
        }, "SentinelRearm").start()
    }

    companion object {
        private const val TAG = "MobileSentinel.Rearm"
        const val ACTION_RING_STARTED = SentinelActiveJobs.ACTION_WAKE_MAIN
    }
}

/** What an app sets with `mobile_sentinel::wake_hook!`. */
object SentinelRearm {
    @JvmStatic
    external fun nativeRearm()
}
