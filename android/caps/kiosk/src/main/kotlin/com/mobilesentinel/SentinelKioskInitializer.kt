package com.mobilesentinel

import android.app.Application
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.util.Log

/**
 * Kiosk library-init `ContentProvider`. Present in the merged manifest ONLY
 * when the consumer enables the `kiosk` feature. Attaches
 * [SentinelKioskController] to the host `Application` lifecycle so it can
 * relaunch the target activity on background events and consume Back.
 *
 * This is the ONLY thing the kiosk module contributes to startup — no
 * alarm/job/firing semantics. A consumer that enables `kiosk` without any
 * other firing surface still gets a working activity lock.
 */
class SentinelKioskInitializer : ContentProvider() {

    companion object {
        private const val TAG = "SentinelKioskInit"
    }

    override fun onCreate(): Boolean {
        val ctx = context ?: return false
        val app = ctx.applicationContext as? Application ?: return false
        SentinelKioskController.attach(app)
        Log.i(TAG, "kiosk initialised; controller attached to Application lifecycle")
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
