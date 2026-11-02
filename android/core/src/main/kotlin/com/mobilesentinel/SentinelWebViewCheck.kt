package com.mobilesentinel

import android.app.Activity
import android.app.AlertDialog
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.util.Log
import android.webkit.WebView
import java.io.File
import kotlin.concurrent.thread

/**
 * Whether the phone's WebView can run the app's UI at all. One that is too old shows
 * a blank page without an error, which on an alarm app means an alarm nobody can
 * stop, so an alarm goes straight to the backup screen then, and opening the app
 * says what is wrong. `min_webview` in `sentinel.toml` sets the version; without it
 * nothing is checked.
 */
object SentinelWebViewCheck {
    private const val TAG = "MobileSentinel.WebView"

    /** Written into the manifest by `build_sentinel`. */
    private const val META_MIN_WEBVIEW = "com.mobilesentinel.min_webview"

    /** Present while the WebView is too old, relative to the files dir. */
    private const val TOO_OLD_FILE = "sentinel/webview_too_old"

    @Volatile
    private var told = false

    /**
     * The last known answer. A ring asks this before it has made a sound, and asking
     * Android instead costs it up to 200 ms on Android 12.
     */
    @JvmStatic
    fun isTooOld(context: Context): Boolean =
        File(SentinelStorage.filesDir(context), TOO_OLD_FILE).exists()

    /** Ask Android and keep the answer. Not for a ring that has yet to make a sound. */
    @JvmStatic
    fun refresh(context: Context): Boolean {
        try {
            val app = context.applicationContext ?: context
            val required = app.packageManager
                .getApplicationInfo(app.packageName, PackageManager.GET_META_DATA)
                .metaData?.getInt(META_MIN_WEBVIEW, 0) ?: 0
            if (required <= 0) return false
            val major = currentMajorVersion()
            Log.i(TAG, "WebView $major, needs $required")
            val tooOld = major in 1 until required
            val file = File(SentinelStorage.filesDir(app), TOO_OLD_FILE)
            if (tooOld) {
                file.parentFile?.mkdirs()
                file.writeText("")
            } else {
                file.delete()
            }
            return tooOld
        } catch (e: Throwable) {
            Log.w(TAG, "check failed: ${e.message}")
            return false
        }
    }

    /** The WebView's major version, 0 when unknown. */
    private fun currentMajorVersion(): Int {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return 0
        val version = WebView.getCurrentWebViewPackage()?.versionName ?: return 0
        return version.substringBefore('.').toIntOrNull() ?: 0
    }

    /** Check once per process, off the UI thread, and offer the update if it is too old. */
    @JvmStatic
    fun tellIfTooOld(activity: Activity) {
        if (told) return
        told = true
        thread(name = "sentinel-webview-check") {
            if (refresh(activity)) {
                activity.runOnUiThread { if (!activity.isFinishing) showNotice(activity) }
            }
        }
    }

    private fun showNotice(activity: Activity) {
        val pkg = WebView.getCurrentWebViewPackage()?.packageName ?: "com.google.android.webview"
        AlertDialog.Builder(activity)
            .setTitle("Android System WebView is too old")
            .setMessage(
                "This app shows its screens with Android System WebView, and the version on " +
                    "this phone cannot run them. Please update it. Alarms still ring until then.",
            )
            .setPositiveButton("Update") { _, _ -> openStore(activity, pkg) }
            .setNegativeButton("Later", null)
            .show()
    }

    private fun openStore(activity: Activity, pkg: String) {
        try {
            activity.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("market://details?id=$pkg")))
        } catch (_: ActivityNotFoundException) {
            activity.startActivity(
                Intent(Intent.ACTION_VIEW, Uri.parse("https://play.google.com/store/apps/details?id=$pkg")),
            )
        }
    }
}
