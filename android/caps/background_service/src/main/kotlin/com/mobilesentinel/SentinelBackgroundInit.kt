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
 * service, and hooks every activity's Back button: Back asks the app what to do
 * (close a picture, go back a screen, move to the background or quit) instead
 * of the system finishing the activity behind the app's back. An app that does
 * not implement the `nativeOnBack` hook keeps the normal system behaviour.
 */
class SentinelBackgroundInit : ContentProvider() {

    companion object {
        /** The app's main activity has been created in this process already. */
        @Volatile
        private var mainCreated = false
    }

    override fun onCreate(): Boolean {
        val app = context?.applicationContext as? Application ?: return false
        app.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
            /**
             * The Rust UI framework (tao) can create its window only once per
             * process: a second activity in the same process panics and takes the
             * process down uncleanly. With the background service the process
             * outlives the activity, so when the system makes a new main activity
             * (it destroyed the old one, a theme/locale change, ...), restart the
             * process on purpose, before the native code runs. The service comes
             * back by itself and the system recreates the activity in the fresh process.
             */
            override fun onActivityPreCreated(activity: Activity, savedInstanceState: Bundle?) {
                val launcher = activity.packageManager
                    .getLaunchIntentForPackage(activity.packageName)?.component?.className
                if (activity.componentName.className != launcher) return
                if (mainCreated) {
                    Log.i("MobileSentinel.Bg", "second main activity in one process: restarting the process")
                    android.os.Process.killProcess(android.os.Process.myPid())
                }
                mainCreated = true
            }

            override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {
                val a = activity as? ComponentActivity ?: return
                a.onBackPressedDispatcher.addCallback(
                    a,
                    object : OnBackPressedCallback(true) {
                        override fun handleOnBackPressed() {
                            if (!SentinelBackground.onBack(a)) {
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
