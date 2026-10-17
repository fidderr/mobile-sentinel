package com.mobilesentinel

import android.app.Activity
import android.app.Application
import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback

/**
 * Library-init provider for the `background_service` capability. It runs when
 * the process starts, including when Android starts the process only for the
 * service, and hooks every activity's Back button: with the background
 * service on, Back asks the app what to do (go back a screen, or move to the
 * background) instead of destroying the activity, so the app is still there,
 * with its connection, when you come back to it.
 */
class SentinelBackgroundInit : ContentProvider() {

    override fun onCreate(): Boolean {
        val app = context?.applicationContext as? Application ?: return false
        app.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
            override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {
                val a = activity as? ComponentActivity ?: return
                a.onBackPressedDispatcher.addCallback(
                    a,
                    object : OnBackPressedCallback(true) {
                        override fun handleOnBackPressed() {
                            if (!SentinelBackground.isEnabled() || !SentinelBackground.onBack(a)) {
                                // Not our business: let the system do what it normally does.
                                isEnabled = false
                                a.onBackPressedDispatcher.onBackPressed()
                                isEnabled = true
                            }
                        }
                    },
                )
            }

            override fun onActivityStarted(activity: Activity) {}
            override fun onActivityResumed(activity: Activity) {}
            override fun onActivityPaused(activity: Activity) {}
            override fun onActivityStopped(activity: Activity) {}
            override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
            override fun onActivityDestroyed(activity: Activity) {}
        })
        Log.i("MobileSentinel.Bg", "background_service initialised")
        return true
    }

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = null
    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = 0
}
