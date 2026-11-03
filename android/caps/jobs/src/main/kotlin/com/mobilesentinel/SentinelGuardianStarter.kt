package com.mobilesentinel

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.util.Log

/**
 * Runs when the `:sentinel` process starts, which is all it is for.
 *
 * Android restarts the ring service after a kill, in a new `:sentinel` with nothing
 * guarding. If MAIN died along with it (ColorOS kills both when an app is swiped away),
 * nobody would bring the UI back, and the alarm would ring on with only a notification.
 */
class SentinelGuardianStarter : ContentProvider() {
    override fun onCreate(): Boolean {
        val app = context?.applicationContext ?: return false
        try {
            if (SentinelActiveJobs.list(app).isNotEmpty()) SentinelJobGuardian.startGuarding(app)
        } catch (e: Throwable) {
            Log.w("SentinelGuardianStarter", "could not look for active jobs: ${e.message}")
        }
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
