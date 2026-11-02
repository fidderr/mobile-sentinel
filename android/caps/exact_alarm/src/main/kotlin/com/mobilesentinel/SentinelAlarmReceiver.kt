package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Universal-SDK alarm receiver — the slim Kotlin shim in `:sentinel`.
 *
 * Runs in the `:sentinel` child process (declared via
 * `android:process=":sentinel"` in the manifest). Its ONLY job:
 *
 * 1. Mark the job for this instance active (instance_id maps to a job id).
 * 2. Start the ring from the plan Rust put on this alarm when it armed it, so
 *    the alarm is heard without waiting for MAIN.
 * 3. Bring MAIN up: [SentinelRearmReceiver] for the state machine, which needs no
 *    UI, and `SentinelJobGuardian.startGuarding()` for the screen.
 *
 * It decides nothing: the state machine and the UI run in MAIN's Rust, which
 * re-derives what to do from the persisted Context on every heads-up.
 */
class SentinelAlarmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        SentinelPrimitives.init(context.applicationContext)

        val pending = goAsync()
        try {
            val app = context.applicationContext
            val instanceId = intent.getStringExtra(EXTRA_INSTANCE_ID).orEmpty()

            Log.i(TAG, "onReceive: instance=$instanceId")
            if (!SentinelBoot.filesAvailable(app)) {
                Log.w(TAG, "files are locked until the first unlock, cannot ring")
                return
            }

            // The OS only keeps the device awake until this returns.
            SentinelRing.holdCpu(app, DELIVERY_WAKE_MS)

            // The ring's watchdog must NEVER activate a job: if the alarm was
            // already dismissed or snoozed, nothing should come back.
            val isWatchdog = instanceId.endsWith(WATCHDOG_SUFFIX)
            val jobId = instanceId.removeSuffix(WATCHDOG_SUFFIX)

            if (isWatchdog) {
                SentinelRing.resumeSaved(app)
            } else if (jobId.isNotBlank()) {
                val active = SentinelActiveJobs.mark(app, jobId)
                // It fired, so the OS no longer holds it: do not replay it at boot.
                SentinelArmedAlarms.forget(context, jobId)
                val plan = intent.getStringExtra(EXTRA_METADATA_JSON)
                if (active && !plan.isNullOrBlank()) {
                    // Armed first, so a service that is refused or dies is retried.
                    SentinelRing.armWatchdog(app)
                    SentinelRing.start(app, jobId, plan)
                }
                if (active && SentinelRing.mainPid(app) == 0) {
                    app.sendBroadcast(
                        Intent(SentinelRearmReceiver.ACTION_RING_STARTED)
                            .setClass(app, SentinelRearmReceiver::class.java),
                    )
                }
            }

            // Start the job guardian polling loop. It will detect the
            // active job, check if MAIN is alive, and either start it
            // or send a heads-up broadcast.
            // Clear heads-up tracking for this job so the guardian
            // re-sends the broadcast (needed for snooze re-fires where
            // the job was already active from the first fire).
            SentinelJobGuardian.clearHeadsUpFor(jobId)
            SentinelJobGuardian.startGuarding(context.applicationContext)
        } catch (t: Throwable) {
            Log.e(TAG, "onReceive failed", t)
        } finally {
            pending.finish()
        }
    }

    companion object {
        const val TAG = "MobileSentinel.AlarmRx"
        const val WATCHDOG_SUFFIX = "#watchdog"
        private const val DELIVERY_WAKE_MS = 15_000L
        const val ACTION = "com.mobilesentinel.ALARM_DISPATCH"
        const val EXTRA_INSTANCE_ID = "instance_id"
        const val EXTRA_METADATA_JSON = "metadata_json"
    }
}
