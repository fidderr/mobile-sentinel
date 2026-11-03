package com.mobilesentinel

import android.content.Context
import android.content.Intent
import android.media.AudioManager
import android.net.Uri
import android.util.Log

/**
 * Phone-call platform primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around an Android telephony API — no orchestration logic, no state
 * machines, no Recipe-specific behavior. Reached from Rust via
 * `crate::features::phone`.
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
     * Reads the audio mode, which needs no permission (`TelephonyManager`'s call
     * state wants `READ_PHONE_STATE` from Android 12 on).
     *
     * @return `true` if a call is ringing or in progress, `false` otherwise.
     */
    @JvmStatic
    fun isInCall(): Boolean {
        val c: Context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val am = c.getSystemService(Context.AUDIO_SERVICE) as? AudioManager
                ?: return false
            am.mode == AudioManager.MODE_RINGTONE || am.mode == AudioManager.MODE_IN_CALL ||
                am.mode == AudioManager.MODE_IN_COMMUNICATION
        } catch (e: Throwable) {
            Log.w(TAG, "isInCall: ${e.message}", e)
            false
        }
    }
}
