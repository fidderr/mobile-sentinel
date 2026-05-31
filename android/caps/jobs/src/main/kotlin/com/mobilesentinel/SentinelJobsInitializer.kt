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

        try {
            val filter = IntentFilter(SentinelJobGuardian.ACTION_JOB_HEADS_UP)
            val receiver = object : BroadcastReceiver() {
                override fun onReceive(context: Context, intent: Intent) {
                    val jobId = intent.getStringExtra(SentinelJobGuardian.EXTRA_JOB_ID) ?: return
                    Log.i(TAG, "JOB_HEADS_UP received for job=$jobId — forwarding to Rust")
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
