package com.mobilesentinel

import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import android.content.Intent
import android.util.Log

/**
 * Permissions capability — runtime permission status/request + the app's
 * system-settings deep link.
 *
 * Lives in the `:sentinel-permissions` module, compiled only when the
 * `permissions` Cargo feature is enabled. Reaches the resumed Activity via the
 * core [SentinelActivityTracker] to host the system dialog.
 *
 * Rust calls INTO these via JNI (`crate::features::permissions`).
 */
object SentinelPermissionPrimitives {
    private const val TAG = "MobileSentinel.Perm"

    /** Request code used for all runtime-permission dialogs we launch. */
    private const val PERMISSION_REQUEST_CODE = 0x5E11 // "SE11" — Sentinel

    /**
     * Request a runtime permission. Returns:
     *  - 0 = already granted
     *  - 1 = not granted (dialog dispatched, or no Activity available)
     *
     * The grant result arrives asynchronously in
     * `Activity.onRequestPermissionsResult`; callers re-check via
     * [checkPermission] or simply retry once the user responds.
     */
    @JvmStatic
    fun requestRuntimePermission(permission: String): Int {
        val c = SentinelPrimitives.getAppContext() ?: return 1
        return try {
            val granted = c.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
            if (granted) {
                0
            } else {
                val activity = SentinelActivityTracker.currentResumedActivity
                if (activity != null) {
                    activity.runOnUiThread {
                        try {
                            activity.requestPermissions(arrayOf(permission), PERMISSION_REQUEST_CODE)
                            Log.i(TAG, "requestRuntimePermission($permission): dialog dispatched")
                        } catch (e: Throwable) {
                            Log.w(TAG, "requestRuntimePermission($permission) dispatch failed: ${e.message}")
                        }
                    }
                } else {
                    Log.w(TAG, "requestRuntimePermission($permission): no resumed Activity")
                }
                1
            }
        } catch (e: Throwable) {
            Log.w(TAG, "requestRuntimePermission($permission): ${e.message}")
            1
        }
    }

    @JvmStatic
    fun checkPermission(permission: String): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            c.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
        } catch (e: Throwable) {
            false
        }
    }

    /**
     * Open this app's system settings detail page
     * (`ACTION_APPLICATION_DETAILS_SETTINGS`) so the user can re-grant a
     * permission they previously denied. Launched from a non-Activity
     * context, so it carries `FLAG_ACTIVITY_NEW_TASK`.
     */
    @JvmStatic
    fun openAppSettings(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val intent = Intent(
                Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                Uri.parse("package:${c.packageName}"),
            ).apply { addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) }
            c.startActivity(intent)
            true
        } catch (e: Throwable) {
            Log.e(TAG, "openAppSettings: ${e.message}", e)
            false
        }
    }
}
