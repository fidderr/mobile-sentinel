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

/**
 * Whether the phone's WebView can run the app's UI at all. One that is too old shows
 * a black page without an error, which on an alarm app means an alarm nobody can
 * stop, so an alarm goes straight to the backup screen then, and opening the app
 * says what is wrong. `min_webview` in `sentinel.toml` sets the version; without it
 * nothing is checked.
 */
object SentinelWebViewCheck {
    private const val TAG = "MobileSentinel.WebView"

    /** Written into the manifest by `build_sentinel`. */
    private const val META_MIN_WEBVIEW = "com.mobilesentinel.min_webview"

    @Volatile
    private var cachedTooOld: Boolean? = null

    @Volatile
    private var told = false

    @JvmStatic
    fun isTooOld(context: Context): Boolean {
        cachedTooOld?.let { return it }
        val tooOld = try {
            val app = context.applicationContext ?: context
            val required = app.packageManager
                .getApplicationInfo(app.packageName, PackageManager.GET_META_DATA)
                .metaData?.getInt(META_MIN_WEBVIEW, 0) ?: 0
            val major = currentMajorVersion()
            Log.i(TAG, "WebView $major, needs $required")
            required > 0 && major in 1 until required
        } catch (e: Throwable) {
            Log.w(TAG, "check failed: ${e.message}")
            false
        }
        cachedTooOld = tooOld
        return tooOld
    }

    /** The WebView's major version, 0 when unknown. */
    private fun currentMajorVersion(): Int {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return 0
        val version = WebView.getCurrentWebViewPackage()?.versionName ?: return 0
        return version.substringBefore('.').toIntOrNull() ?: 0
    }

    /** Tell the user, once per process, and offer the update. */
    @JvmStatic
    fun tellIfTooOld(activity: Activity) {
        if (told || !isTooOld(activity)) return
        told = true
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
