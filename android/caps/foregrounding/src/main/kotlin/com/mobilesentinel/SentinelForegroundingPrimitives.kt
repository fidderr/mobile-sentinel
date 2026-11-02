package com.mobilesentinel

import android.app.Activity
import android.app.Application
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.view.KeyEvent
import android.view.Window
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher

/**
 * Foregrounding capability — finish the app's activity (close after
 * dismiss/snooze).
 *
 * Lives in the `:sentinel-foregrounding` module, compiled only when the
 * `foregrounding` Cargo feature is enabled. Uses only the core
 * [SentinelActivityTracker] (no alarm/kiosk dependency). Callers that also
 * run a kiosk should disable it first, otherwise the kiosk relaunch watchdog
 * brings the activity straight back.
 *
 * Rust calls INTO this via JNI (`crate::features::foregrounding`).
 */
object SentinelForegroundingPrimitives {
    private const val TAG = "MobileSentinel.Fgnd"

    @Volatile
    private var routing = false

    /**
     * Send Back to Rust from now on, instead of letting the system finish the
     * activity. A finished activity ends the whole process in a Dioxus app, and what
     * Back does differs per Android version otherwise: from 13 on, with
     * `enableOnBackInvokedCallback`, the key never reaches the activity at all.
     */
    @JvmStatic
    fun routeBack(): Boolean {
        val app = SentinelPrimitives.getAppContext() as? Application ?: return false
        if (routing) return true
        routing = true
        SentinelActivityTracker.currentResumedActivity?.let { activity ->
            activity.runOnUiThread { catchBack(activity) }
        }
        app.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
            override fun onActivityResumed(activity: Activity) = catchBack(activity)
            override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}
            override fun onActivityStarted(activity: Activity) {}
            override fun onActivityPaused(activity: Activity) {}
            override fun onActivityStopped(activity: Activity) {}
            override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
            override fun onActivityDestroyed(activity: Activity) {}
        })
        return true
    }

    private val caught = java.util.WeakHashMap<Activity, Boolean>()

    /** Only the app's own screen: a scanner or file picker keeps its Back. */
    private fun catchBack(activity: Activity) {
        val launcher = activity.packageManager
            .getLaunchIntentForPackage(activity.packageName)?.component?.className
        if (activity.componentName.className != launcher || caught.put(activity, true) == true) return
        try {
            activity.window?.let { window ->
                window.callback?.let { window.callback = BackKey(it) }
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                activity.onBackInvokedDispatcher.registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                    OnBackInvokedCallback { sendBack() },
                )
            }
        } catch (e: Throwable) {
            Log.w(TAG, "catching Back failed: ${e.message}")
        }
    }

    /** Below Android 13, where Back arrives as a key. */
    private class BackKey(private val base: Window.Callback) : Window.Callback by base {
        override fun dispatchKeyEvent(event: KeyEvent): Boolean {
            if (event.keyCode != KeyEvent.KEYCODE_BACK) return base.dispatchKeyEvent(event)
            if (event.action == KeyEvent.ACTION_UP && !event.isCanceled) sendBack()
            return true
        }
    }

    private fun sendBack() {
        try {
            nativeOnBack()
        } catch (e: UnsatisfiedLinkError) {
            Log.w(TAG, "nativeOnBack not linked: ${e.message}")
        }
    }

    @JvmStatic
    private external fun nativeOnBack()

    /** To the background, keeping the activity and the process alive. */
    @JvmStatic
    fun moveToBack(): Boolean {
        val activity = SentinelActivityTracker.currentResumedActivity ?: return false
        activity.runOnUiThread { activity.moveTaskToBack(true) }
        return true
    }

    @JvmStatic
    fun finishActivity(): Boolean {
        val activity = SentinelActivityTracker.currentResumedActivity ?: run {
            Log.w(TAG, "finishActivity: no resumed Activity")
            return false
        }
        return try {
            activity.runOnUiThread {
                try {
                    activity.finishAndRemoveTask()
                    Log.i(TAG, "finishActivity: finishAndRemoveTask dispatched")
                } catch (e: Throwable) {
                    Log.w(TAG, "finishActivity dispatch failed: ${e.message}")
                }
            }
            true
        } catch (e: Throwable) {
            Log.e(TAG, "finishActivity: ${e.message}", e)
            false
        }
    }
}
