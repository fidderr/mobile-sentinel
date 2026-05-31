package com.mobilesentinel

import android.location.Address
import android.location.Geocoder
import android.util.Log

/**
 * Geocoding primitives for the universal SDK.
 *
 * Rust calls INTO these functions via JNI, synchronously, and blocks on the
 * result. Each method is a thin wrapper around [android.location.Geocoder] —
 * no orchestration, no state, no Recipe-specific behavior. The Rust side
 * returns a `Coordinate` struct; here we flatten to a single `"lat,lng"`
 * string because JNI returns one `String`. The Rust wrapper parses it back
 * into coordinates.
 *
 * Both Geocoder overloads used here (`getFromLocationName` /
 * `getFromLocation`) are the synchronous variants. They are deprecated on
 * API 33+ in favor of the async listener overloads, but they still function
 * and are required because Rust invokes these methods synchronously and
 * blocks the calling thread. The async listener overloads cannot satisfy
 * that contract, so deprecation is suppressed at the method level.
 *
 * Every method is total: any failure (null context, no result, thrown
 * exception) returns `""`. These methods never throw.
 */
object SentinelMapsPrimitives {
    private const val TAG = "MobileSentinel.Maps"

    /**
     * Forward geocode an address to coordinates.
     *
     * @param address free-form address / place name to resolve.
     * @return `"lat,lng"` (e.g. `"52.3676,4.9041"`) for the first match,
     *         or `""` on failure or when no result is found.
     */
    @JvmStatic
    @Suppress("DEPRECATION")
    fun geocode(address: String): String {
        val context = SentinelPrimitives.getAppContext() ?: return ""
        return try {
            val geocoder = Geocoder(context)
            val results: List<Address>? = geocoder.getFromLocationName(address, 1)
            if (results.isNullOrEmpty()) {
                return ""
            }
            val first = results[0]
            "${first.latitude},${first.longitude}"
        } catch (e: Throwable) {
            Log.w(TAG, "geocode failed: ${e.message}", e)
            ""
        }
    }

    /**
     * Reverse geocode coordinates to a human-readable address line.
     *
     * @param latitude  latitude in decimal degrees.
     * @param longitude longitude in decimal degrees.
     * @return the first address line for the location, or `""` on failure,
     *         when no result is found, or when the address line is null.
     */
    @JvmStatic
    @Suppress("DEPRECATION")
    fun reverseGeocode(latitude: Double, longitude: Double): String {
        val context = SentinelPrimitives.getAppContext() ?: return ""
        return try {
            val geocoder = Geocoder(context)
            val results: List<Address>? = geocoder.getFromLocation(latitude, longitude, 1)
            if (results.isNullOrEmpty()) {
                return ""
            }
            results[0].getAddressLine(0) ?: ""
        } catch (e: Throwable) {
            Log.w(TAG, "reverseGeocode failed: ${e.message}", e)
            ""
        }
    }
}
