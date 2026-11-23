package com.mobilesentinel

import android.app.Application
import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.util.Log

/**
 * Jobs library-init `ContentProvider`. Present in the merged manifest ONLY
 * when the consumer enables the `jobs` feature.
 *
 * Runs in MAIN when the process starts. The `JOB_HEADS_UP` broadcast of the
 * `:sentinel` job guardian is heard by [SentinelJobHeadsUpReceiver], declared in
 * the manifest: a receiver registered here at run time would be queued while MAIN
 * is cached (Android 14+).
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

        Log.i(TAG, "jobs initialised")
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
