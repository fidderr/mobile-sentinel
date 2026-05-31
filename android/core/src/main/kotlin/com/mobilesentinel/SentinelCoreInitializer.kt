package com.mobilesentinel

import android.app.Application
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.util.Log

/**
 * Core library-init `ContentProvider`. Runs during `Application.onCreate`
 * (the AndroidX-Startup / Firebase / WorkManager pattern) so consumers don't
 * have to subclass `Application`.
 *
 * This is the ONLY component `:sentinel-core` contributes to a consumer's
 * manifest, and it does only irreducible kernel work — no alarm/firing/kiosk
 * semantics (those live in their own per-feature modules' initializers, each
 * present only when that capability is enabled):
 *
 * 1. [SentinelPrimitives.init] — set the app context so the JNI primitives
 *    layer works before any Rust call.
 * 2. [SentinelActivityTracker.attach] — track the resumed Activity for
 *    capabilities that need one (permissions, screen pinning, scanner).
 */
class SentinelCoreInitializer : ContentProvider() {

    companion object {
        private const val TAG = "SentinelCoreInit"
    }

    override fun onCreate(): Boolean {
        val ctx = context ?: return false
        val app = ctx.applicationContext as? Application ?: return false

        // Universal SDK primitives — must run before any JNI primitive call
        // (audio play, asset copy, permission checks, etc.).
        SentinelPrimitives.init(app)

        // Track the resumed Activity for capabilities that need one.
        SentinelActivityTracker.attach(app)

        Log.i(TAG, "mobile-sentinel core initialised (primitives + activity tracker)")
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

    override fun delete(
        uri: Uri,
        selection: String?,
        selectionArgs: Array<out String>?,
    ): Int = 0

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<out String>?,
    ): Int = 0
}
