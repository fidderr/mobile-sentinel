package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.util.Log

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
        Log.i(TAG, "JOB_HEADS_UP received for job=$jobId — forwarding to Rust")
        // MAIN can be alive with no UI at all — e.g. Android restarted only the sticky
        // foreground service after the app was killed, or the app sits in the background.
        // Rust may not run in that process, so the heads-up alone would go nowhere: bring the
        // job's activity up, whose startup re-engages the job.
        if (SentinelActivityTracker.currentResumedActivity == null) {
            launchJobActivity(context, jobId)
        }
        try {
            SentinelBridge.onJobHeadsUp(jobId)
        } catch (e: UnsatisfiedLinkError) {
            Log.w(TAG, "nativeOnJobHeadsUp not linked yet")
        }
    }

    /** Start the activity named in [jobId]'s payload (`activity_fqcn`), if the job is active. */
    private fun launchJobActivity(context: Context, jobId: String) {
        // It would only cover the backup screen, the one way out.
        if (SentinelRing.uiFailed(context)) return
        try {
            val job = SentinelActiveJobs.list(context).firstOrNull { it.id == jobId } ?: return
            val fqcn = SentinelActiveJobs.activityOf(job) ?: return
            // Until the first unlock Android does not start it: the ring service asks for that.
            if (!SentinelBoot.screenCanStart(context, fqcn)) return
            val launch = Intent().apply {
                component = ComponentName(context.packageName, fqcn)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or
                    Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or
                    Intent.FLAG_ACTIVITY_SINGLE_TOP
            }
            context.startActivity(launch)
            Log.i(TAG, "no UI in MAIN — launched $fqcn for job=$jobId")
        } catch (e: Throwable) {
            Log.w(TAG, "launchJobActivity($jobId) failed: ${e.message}")
        }
    }

    private companion object {
        const val TAG = "SentinelJobsInit"
    }
}
