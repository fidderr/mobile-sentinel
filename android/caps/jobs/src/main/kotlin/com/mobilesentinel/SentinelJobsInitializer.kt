package com.mobilesentinel

import android.app.Application
import android.content.BroadcastReceiver
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.util.Log

/**
 * Jobs library-init `ContentProvider`. Present in the merged manifest ONLY
 * when the consumer enables the `jobs` feature.
 *
 * Registers a runtime broadcast receiver for `JOB_HEADS_UP` from the
 * `:sentinel` job guardian. When MAIN is already alive and a job fires, the
 * guardian sends this broadcast instead of starting the activity; we forward
 * the job id to Rust, which runs the job. No alarm/kiosk semantics here.
 */
class SentinelJobsInitializer : ContentProvider() {

    companion object {
        private const val TAG = "SentinelJobsInit"
    }

    override fun onCreate(): Boolean {
        val ctx = context ?: return false
        val app = ctx.applicationContext as? Application ?: return false

        // A ring that ended without its service (force-stop) left the volume up.
        SentinelRing.restoreVolumeIfIdle(app)

        try {
            val filter = IntentFilter(SentinelJobGuardian.ACTION_JOB_HEADS_UP)
            val receiver = object : BroadcastReceiver() {
                override fun onReceive(context: Context, intent: Intent) {
                    val jobId = intent.getStringExtra(SentinelJobGuardian.EXTRA_JOB_ID) ?: return
                    Log.i(TAG, "JOB_HEADS_UP received for job=$jobId — forwarding to Rust")
                    // MAIN can be alive with no UI at all — e.g. Android
                    // restarted only the sticky foreground service after the
                    // app was killed. Rust is not running in that process, so
                    // the heads-up alone would go nowhere: bring the job's
                    // activity up, whose startup re-engages the job.
                    if (SentinelActivityTracker.currentResumedActivity == null) {
                        launchJobActivity(context, jobId)
                    }
                    try {
                        SentinelBridge.onJobHeadsUp(jobId)
                    } catch (e: UnsatisfiedLinkError) {
                        Log.w(TAG, "nativeOnJobHeadsUp not linked yet")
                    }
                }
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                app.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
            } else {
                app.registerReceiver(receiver, filter)
            }
        } catch (e: Throwable) {
            Log.w(TAG, "Failed to register JOB_HEADS_UP receiver: ${e.message}")
        }

        Log.i(TAG, "jobs initialised; heads-up receiver registered")
        return true
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
                component = android.content.ComponentName(context.packageName, fqcn)
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

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor? = null

    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0
    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<out String>?,
    ): Int = 0
}
