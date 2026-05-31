package com.mobilesentinel

import android.util.Log

/**
 * Dismiss-guard primitives.
 *
 * Rust calls INTO these functions via JNI. Mirrors the intent of the
 * dismiss-guard primitive.
 *
 * In this SDK the real dismiss-prevention is performed by the kiosk
 * controller. This guard is therefore a lightweight in-memory state flag
 * plus a best-effort delegation to screen pinning: it does not own any
 * window flags itself. The flag is process-local.
 *
 * Every method is `@JvmStatic`, wrapped in try/catch, logs via [Log.w],
 * never throws, and returns a safe fallback.
 */
object SentinelDismissGuardPrimitives {
    private const val TAG = "MobileSentinel.DismissGuard"

    /** Process-local dismiss-guard state flag. */
    @Volatile
    private var active = false

    /**
     * Activates the dismiss guard: sets the state flag and makes a
     * best-effort attempt to pin the screen (result ignored).
     *
     * Returns true. Safe fallback: false (only on unexpected error).
     */
    @JvmStatic
    fun activate(): Boolean {
        return try {
            active = true
            // Best-effort screen pin; ignore the result. Pinning requires
            // an Activity context and may legitimately fail here.
            SentinelScreenPinPrimitives.pin()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "activate: ${e.message}")
            false
        }
    }

    /**
     * Deactivates the dismiss guard: clears the state flag and makes a
     * best-effort attempt to unpin the screen (result ignored).
     *
     * Returns true. Safe fallback: false (only on unexpected error).
     */
    @JvmStatic
    fun deactivate(): Boolean {
        return try {
            active = false
            // Best-effort screen unpin; ignore the result.
            SentinelScreenPinPrimitives.unpin()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "deactivate: ${e.message}")
            false
        }
    }

    /**
     * Returns the current dismiss-guard state flag.
     *
     * Safe fallback: false.
     */
    @JvmStatic
    fun isActive(): Boolean {
        return try {
            active
        } catch (e: Throwable) {
            Log.w(TAG, "isActive: ${e.message}")
            false
        }
    }
}
