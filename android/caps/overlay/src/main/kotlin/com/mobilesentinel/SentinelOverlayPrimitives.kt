package com.mobilesentinel

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.util.Log

/**
 * Overlay capability — "draw over other apps" (SYSTEM_ALERT_WINDOW).
 *
 * Lives in the `:sentinel-overlay` module, compiled only when the `overlay`
 * Cargo feature is enabled. Lets the consumer check / request the overlay
 * permission used to force a firing UI to the foreground.
 *
 * Rust calls INTO these via JNI (`crate::features::overlay`).
 */
object SentinelOverlayPrimitives {
    private const val TAG = "MobileSentinel.Overlay"

    /** Whether the "draw over other apps" permission is granted. */
    @JvmStatic
    fun isGranted(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            Build.VERSION.SDK_INT < Build.VERSION_CODES.M || Settings.canDrawOverlays(c)
        } catch (e: Throwable) {
            Log.w(TAG, "isGranted: ${e.message}")
            false
        }
    }

    /** Launch the overlay-permission settings screen if not already granted. */
    @JvmStatic
    fun request(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && !Settings.canDrawOverlays(c)) {
                val intent = Intent(
                    Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                    Uri.parse("package:${c.packageName}"),
                ).apply { addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) }
                c.startActivity(intent)
            }
            true
        } catch (e: Throwable) {
            Log.e(TAG, "request: ${e.message}", e)
            false
        }
    }
}
