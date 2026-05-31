package com.mobilesentinel

import android.content.Context
import android.location.Location
import android.location.LocationManager
import android.util.Log

/**
 * Universal SDK location primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around [LocationManager] — no orchestration logic, no state machines, no
 * Recipe-specific behavior. The Rust side flattens the result into a
 * `"lat,lng"` string.
 *
 * Permissions: [getCurrent] reads the last-known fix, which requires
 * `ACCESS_FINE_LOCATION` or `ACCESS_COARSE_LOCATION`. When the permission
 * is not granted the platform throws a [SecurityException]; that is
 * expected and is caught here so the primitive degrades to `""` rather
 * than crashing the caller.
 *
 * Contract: every method wraps its body in try/catch, logs via
 * `android.util.Log.w`, and returns the empty / false fallback. These
 * methods never throw.
 *
 * Continuous updates (`on_change` / `stop_updates`) are intentionally NOT
 * implemented here — callback wiring is handled separately.
 */
object SentinelLocationPrimitives {
    private const val TAG = "MobileSentinel.Location"

    /**
     * Returns the device's best last-known coordinate as `"lat,lng"`.
     *
     * Tries the GPS provider first, then falls back to the NETWORK
     * provider. Returns `""` when no fix is available, the context is not
     * yet initialised, the permission is missing, or any error occurs.
     */
    @JvmStatic
    fun getCurrent(): String {
        val ctx: Context = SentinelPrimitives.getAppContext() ?: return ""
        return try {
            val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as? LocationManager
                ?: return ""

            val location: Location? =
                lm.getLastKnownLocation(LocationManager.GPS_PROVIDER)
                    ?: lm.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)

            if (location != null) {
                "${location.latitude},${location.longitude}"
            } else {
                ""
            }
        } catch (e: SecurityException) {
            // Expected when ACCESS_FINE_LOCATION / ACCESS_COARSE_LOCATION
            // has not been granted. Degrade gracefully.
            Log.w(TAG, "getCurrent: location permission not granted: ${e.message}")
            ""
        } catch (e: Throwable) {
            Log.w(TAG, "getCurrent: ${e.message}", e)
            ""
        }
    }

    /**
     * Returns whether the GPS provider is currently enabled. Returns
     * `false` when the context is not initialised or any error occurs.
     */
    @JvmStatic
    fun isEnabled(): Boolean {
        val ctx: Context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as? LocationManager
                ?: return false
            lm.isProviderEnabled(LocationManager.GPS_PROVIDER)
        } catch (e: Throwable) {
            Log.w(TAG, "isEnabled: ${e.message}", e)
            false
        }
    }
}
