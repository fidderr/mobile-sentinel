package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.os.SystemClock
import android.util.Log
import java.io.File
import java.util.concurrent.ConcurrentHashMap

/**
 * MAIN's end of the job guardian's `JOB_HEADS_UP` broadcast.
 *
 * Declared in the manifest, not registered at run time: from Android 14 on a
 * context-registered receiver of an app in the cached state is queued, and the
 * app is frozen 10 seconds after it went cached, so a ring that started while
 * the app sat in the background was not heard of by MAIN, and its screen did not
 * come up. A manifest-declared receiver is not queued: the system takes the app
 * out of the cached state for it.
 *
 * When MAIN is alive, a job fires and the guardian sends the broadcast;
 * we forward the job id to Rust, which runs the job. No alarm/kiosk semantics here.
 */
class SentinelJobHeadsUpReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        val jobId = intent.getStringExtra(SentinelJobGuardian.EXTRA_JOB_ID) ?: return
        if (heldBackForTest(context)) {
            Log.w(TAG, "JOB_HEADS_UP for job=$jobId held back on purpose (debug build)")
            return
        }
        Log.i(TAG, "JOB_HEADS_UP received for job=$jobId — forwarding to Rust")
        // MAIN can be alive with no UI at all — e.g. Android restarted only the sticky
        // foreground service after the app was killed, or the app sits in the background.
        // Rust may not run in that process, so the heads-up alone would go nowhere: bring the
        // job's activity up, whose startup re-engages the job.
        var screen = SentinelActivityTracker.currentResumedActivity != null
        if (!screen) screen = launchJobActivity(context, jobId)
        try {
            SentinelBridge.onJobHeadsUp(jobId)
        } catch (e: UnsatisfiedLinkError) {
            Log.w(TAG, "nativeOnJobHeadsUp not linked yet")
        }
        // The guardian looks for this: no sign, and it brings the screen up itself.
        if (screen) SentinelJobGuardian.noteHeadsUpTaken(context, jobId)
    }

    /**
     * Start the activity named in [jobId]'s payload (`activity_fqcn`), if the job is active.
     * Returns whether it was started, or has been started a moment ago: a second heads-up (a
     * late one that was queued, or the guardian's own) must not start the screen again.
     */
    private fun launchJobActivity(context: Context, jobId: String): Boolean {
        // It would only cover the backup screen, the one way out.
        if (SentinelRing.uiFailed(context)) return false
        try {
            val job = SentinelActiveJobs.list(context).firstOrNull { it.id == jobId } ?: return false
            val fqcn = SentinelActiveJobs.activityOf(job) ?: return false
            // Until the first unlock Android does not start it: the ring service asks for that.
            if (!SentinelBoot.screenCanStart(context, fqcn)) return false
            val now = SystemClock.elapsedRealtime()
            val before = launchedAt[jobId]
            if (before != null && now - before < LAUNCH_ONCE_MS) return true
            launchedAt[jobId] = now
            val launch = Intent().apply {
                component = ComponentName(context.packageName, fqcn)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or
                    Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or
                    Intent.FLAG_ACTIVITY_SINGLE_TOP
            }
            context.startActivity(launch)
            Log.i(TAG, "no UI in MAIN — launched $fqcn for job=$jobId")
            return true
        } catch (e: Throwable) {
            Log.w(TAG, "launchJobActivity($jobId) failed: ${e.message}")
            return false
        }
    }

    /**
     * For the device tests of the guardian's fallback: a debug build with the file
     * `sentinel/debug_hold_heads_up` ignores the broadcast. A release build never does.
     */
    private fun heldBackForTest(context: Context): Boolean {
        val debuggable = context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        return debuggable && File(SentinelStorage.filesDir(context), HOLD_FILE).exists()
    }

    private companion object {
        const val TAG = "SentinelJobsInit"
        const val HOLD_FILE = "sentinel/debug_hold_heads_up"
        const val LAUNCH_ONCE_MS = 3_000L

        /** When this process last started the screen of a job, by job id. */
        val launchedAt = ConcurrentHashMap<String, Long>()
    }
}
