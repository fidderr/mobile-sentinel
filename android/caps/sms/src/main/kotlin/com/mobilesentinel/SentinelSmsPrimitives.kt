package com.mobilesentinel

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.telephony.SmsManager
import android.util.Log

/**
 * SMS platform primitives for the universal SDK.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around an Android API — no orchestration logic, no Recipe-specific
 * behavior. The application context is obtained from
 * [SentinelPrimitives.getAppContext]; nothing here owns state.
 *
 * Every method is wrapped in try/catch and never throws: a failure is
 * surfaced as `false` and logged under the [TAG] tag.
 */
object SentinelSmsPrimitives {
    private const val TAG = "MobileSentinel.Sms"

    /**
     * Send a text message to [number] with body [message].
     *
     * Resolves an [SmsManager] (API 31+ via the system service, older
     * releases via the deprecated [SmsManager.getDefault]) and calls
     * `sendTextMessage(number, null, message, null, null)`.
     *
     * @return true if the send was dispatched, false on any failure
     *         (no context, no SmsManager, or an exception).
     */
    @JvmStatic
    fun send(number: String, message: String): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val smsManager = resolveSmsManager(context) ?: return false
            smsManager.sendTextMessage(number, null, message, null, null)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "send failed: ${e.message}", e)
            false
        }
    }

    /**
     * Report whether the device exposes telephony hardware, i.e. whether
     * it can send SMS at all.
     *
     * @return true when [PackageManager.FEATURE_TELEPHONY]
     *         (`"android.hardware.telephony"`) is present, false otherwise
     *         or on any exception.
     */
    @JvmStatic
    fun isAvailable(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            context.packageManager.hasSystemFeature(PackageManager.FEATURE_TELEPHONY)
        } catch (e: Throwable) {
            Log.w(TAG, "isAvailable failed: ${e.message}", e)
            false
        }
    }

    /**
     * Obtain an [SmsManager] using whichever path resolves on this
     * platform. On API 31+ the system-service lookup is preferred; on
     * older releases (or if the modern lookup fails) it falls back to the
     * deprecated [SmsManager.getDefault]. Returns null if neither works.
     */
    private fun resolveSmsManager(context: Context): SmsManager? {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            try {
                val manager = context.getSystemService(SmsManager::class.java)
                if (manager != null) return manager
            } catch (e: Throwable) {
                Log.w(TAG, "getSystemService(SmsManager) failed: ${e.message}", e)
            }
        }
        return try {
            @Suppress("DEPRECATION")
            SmsManager.getDefault()
        } catch (e: Throwable) {
            Log.w(TAG, "SmsManager.getDefault failed: ${e.message}", e)
            null
        }
    }
}
