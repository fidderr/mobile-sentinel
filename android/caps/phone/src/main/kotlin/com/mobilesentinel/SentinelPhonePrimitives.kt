package com.mobilesentinel

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.telephony.TelephonyManager
import android.util.Log

/**
 * Phone-call platform primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around an Android telephony API — no orchestration logic, no state
 * machines, no Recipe-specific behavior. Reached from Rust via
 * `primitives_ext::phone_*`.
 *
 * The application context is obtained from [SentinelPrimitives.getAppContext]
 * so these helpers work without an Activity reference (e.g. when called
 * from a service or the guardian process). Every method is defensive: it
 * catches all failures, logs a warning, and returns `false` rather than
 * propagating an exception across the JNI boundary.
 */
object SentinelPhonePrimitives {
    private const val TAG = "MobileSentinel.Phone"

    /**
     * Open the system dialer pre-filled with [number].
     *
     * Uses `ACTION_DIAL`, which only opens the dialer and does NOT place
     * the call, so no `CALL_PHONE` permission is required. Because the
     * call originates from a non-Activity context, `FLAG_ACTIVITY_NEW_TASK`
     * is added.
     *
     * @return `true` if the dialer was launched, `false` on any failure.
     */
    @JvmStatic
    fun dial(number: String): Boolean {
        val c: Context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val intent = Intent(Intent.ACTION_DIAL, Uri.parse("tel:$number")).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            c.startActivity(intent)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "dial($number): ${e.message}", e)
            false
        }
    }

    /**
     * Report whether a phone call is currently active (ringing or off-hook).
     *
     * Reads `TelephonyManager.getCallState()` and returns `true` when the
     * state is not `CALL_STATE_IDLE` (0). On newer API levels reading the
     * call state may require `READ_PHONE_STATE`; a [SecurityException] is
     * caught and treated as "not in a call".
     *
     * @return `true` if a call is ringing or off-hook, `false` otherwise
     *   (including when state is unavailable or permission is denied).
     */
    @JvmStatic
    fun isInCall(): Boolean {
        val c: Context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val tm = c.getSystemService(Context.TELEPHONY_SERVICE) as? TelephonyManager
                ?: return false
            @Suppress("DEPRECATION")
            val state = tm.callState
            state != TelephonyManager.CALL_STATE_IDLE
        } catch (e: SecurityException) {
            Log.w(TAG, "isInCall: missing READ_PHONE_STATE: ${e.message}")
            false
        } catch (e: Throwable) {
            Log.w(TAG, "isInCall: ${e.message}", e)
            false
        }
    }
}
