package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import org.json.JSONObject
import java.io.File

/**
 * Universal-SDK alarm receiver — the slim Kotlin shim in `:sentinel`.
 *
 * Runs in the `:sentinel` child process (declared via
 * `android:process=":sentinel"` in the manifest). Its ONLY job:
 *
 * 1. Read the job file for this instance (instance_id maps to a job id).
 * 2. Set `status` to "active" if it was "pending".
 * 3. Call `SentinelJobGuardian.startGuarding()`.
 *
 * That's it — no starting activities, no writing trigger files, no deciding
 * what kind of wake this is. ALL logic (audio, kiosk, FGS, state machine)
 * runs in MAIN's Rust, which re-derives what to do from the persisted
 * Context on every heads-up (LoadContext-on-every-trigger). The receiver
 * never interprets a trigger — that decision belongs to Rust.
 */
class SentinelAlarmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        SentinelPrimitives.init(context.applicationContext)

        val pending = goAsync()
        try {
            val instanceId = intent.getStringExtra(EXTRA_INSTANCE_ID).orEmpty()

            Log.i(TAG, "onReceive: instance=$instanceId")

            // A firing watchdog (`<instance>#watchdog`, armed by MAIN while an
            // alarm rings) only re-checks the guardian. It must NEVER activate
            // a job: if the alarm was already dismissed/snoozed, the job is
            // pending and the guardian correctly does nothing.
            val isWatchdog = instanceId.endsWith(WATCHDOG_SUFFIX)
            val jobId = instanceId.removeSuffix(WATCHDOG_SUFFIX)

            // Activate the job file (set status to "active" if pending).
            if (!isWatchdog && jobId.isNotBlank()) {
                activateJob(context, jobId)
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

    /**
     * Read the job file and set status to "active" if it was "pending".
     */
    private fun activateJob(context: Context, jobId: String) {
        try {
            val file = File(context.applicationContext.filesDir, "sentinel/jobs/$jobId.json")
            if (!file.exists()) {
                Log.w(TAG, "activateJob: no job file for '$jobId'")
                return
            }
            val json = JSONObject(file.readText())
            val status = json.optString("status", "")
            if (status == "pending") {
                json.put("status", "active")
                writeAtomically(file, json.toString(2))
                Log.i(TAG, "activateJob: '$jobId' pending → active")
            } else {
                Log.d(TAG, "activateJob: '$jobId' already '$status'")
            }
        } catch (e: Throwable) {
            Log.w(TAG, "activateJob failed for '$jobId': ${e.message}")
        }
    }

    /**
     * Replace [file] atomically (temp file + rename). MAIN's Rust polls and
     * rewrites these files from another process, so a plain `writeText`
     * (truncate-then-write) could be observed half-written. The temp name ends
     * in `.tmp`, so readers filtering on the `json` extension never see it.
     */
    private fun writeAtomically(file: File, content: String) {
        val tmp = File(file.parentFile, "${file.name}.${android.os.Process.myPid()}.tmp")
        try {
            tmp.writeText(content)
            java.nio.file.Files.move(
                tmp.toPath(),
                file.toPath(),
                java.nio.file.StandardCopyOption.ATOMIC_MOVE,
                java.nio.file.StandardCopyOption.REPLACE_EXISTING,
            )
        } finally {
            if (tmp.exists()) tmp.delete()
        }
    }

    companion object {
        const val TAG = "MobileSentinel.AlarmRx"
        /** Mirrors `WATCHDOG_SUFFIX` in Rust (`recipes::alarm_kit::alarm_class`). */
        const val WATCHDOG_SUFFIX = "#watchdog"
        const val ACTION = "com.mobilesentinel.ALARM_DISPATCH"
        const val EXTRA_INSTANCE_ID = "instance_id"
        const val EXTRA_METADATA_JSON = "metadata_json"
    }
}
