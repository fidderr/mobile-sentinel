package com.mobilesentinel

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.util.Log

/**
 * Network connectivity primitives.
 *
 * Rust calls INTO these functions via JNI to inspect the current network
 * state. Like every other primitive object, these are thin wrappers around
 * Android APIs — no orchestration, no state machines, no Recipe-specific
 * behavior.
 *
 * This implementation uses the modern [ConnectivityManager.getActiveNetwork]
 * + [ConnectivityManager.getNetworkCapabilities] API (API 23+), in contrast
 * to the legacy `getActiveNetworkInfo` path used by the raw-JNI Rust backend.
 * The integer mappings returned to Rust are identical so either backend is
 * interchangeable from the Rust side.
 *
 * Connection-type integers (matching the Rust `ConnectionType` enum):
 *   - 0 = None
 *   - 1 = Wifi
 *   - 2 = Cellular
 *
 * Every public method wraps its body in try/catch, logs failures via
 * [Log.w], and returns a safe default (`false` / `0`). These methods never
 * throw — JNI callers can rely on that.
 *
 * Note: on-change (NetworkCallback) registration is handled separately and is
 * intentionally not implemented here.
 */
object SentinelNetworkPrimitives {
    private const val TAG = "MobileSentinel.Network"

    /**
     * Returns `true` when the active network has validated internet access.
     *
     * Uses [NetworkCapabilities.NET_CAPABILITY_INTERNET] together with
     * [NetworkCapabilities.NET_CAPABILITY_VALIDATED]; if the validated
     * capability is unavailable for any reason, falls back to requiring only
     * the internet capability. Returns `false` when there is no app context,
     * no active network, or no capabilities.
     */
    @JvmStatic
    fun isConnected(): Boolean {
        return try {
            val ctx = SentinelPrimitives.getAppContext() ?: return false
            val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return false

            val network = cm.activeNetwork ?: return false
            val caps = cm.getNetworkCapabilities(network) ?: return false

            val hasInternet = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            val isValidated = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)

            // Prefer validated internet; fall back to plain internet capability.
            hasInternet && isValidated || hasInternet
        } catch (e: Throwable) {
            Log.w(TAG, "isConnected: ${e.message}", e)
            false
        }
    }

    /**
     * Returns the active connection type as an integer:
     *   - 1 when the active network is over Wi-Fi
     *   - 2 when the active network is cellular
     *   - 0 otherwise (no network, or another transport)
     */
    @JvmStatic
    fun connectionType(): Int {
        return try {
            val ctx = SentinelPrimitives.getAppContext() ?: return 0
            val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return 0

            val network = cm.activeNetwork ?: return 0
            val caps = cm.getNetworkCapabilities(network) ?: return 0

            when {
                caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> 1
                caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> 2
                else -> 0
            }
        } catch (e: Throwable) {
            Log.w(TAG, "connectionType: ${e.message}", e)
            0
        }
    }
}
