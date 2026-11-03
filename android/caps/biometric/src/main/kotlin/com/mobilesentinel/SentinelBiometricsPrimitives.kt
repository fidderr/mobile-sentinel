package com.mobilesentinel

import android.util.Log
import androidx.biometric.BiometricManager

/**
 * Universal SDK biometric primitives.
 *
 * Thin JNI-facing wrappers around the AndroidX [BiometricManager] /
 * `PackageManager` APIs. Mirrors the Rust `AndroidBiometrics` backend
 * so the same decisions can be made from either side of the bridge — no
 * orchestration logic here.
 *
 * Threading: every method is `@JvmStatic`, stateless, and never throws.
 * Each body returns a safe fallback on any failure (`false` / `0`).
 */
object SentinelBiometricsPrimitives {
    private const val TAG = "MobileSentinel.Biometrics"

    /**
     * True iff a strong biometric is enrolled and usable right now.
     *
     * Uses `BiometricManager.from(context).canAuthenticate(BIOMETRIC_STRONG)`
     * and returns true only when it equals `BIOMETRIC_SUCCESS`.
     */
    @JvmStatic
    fun isAvailable(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val result = BiometricManager.from(context)
                .canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG)
            result == BiometricManager.BIOMETRIC_SUCCESS
        } catch (e: Throwable) {
            Log.w(TAG, "isAvailable failed: ${e.message}", e)
            false
        }
    }

    /**
     * Classify the available biometric hardware.
     *
     * Returns `2` (Face) when the device reports face hardware, else `1`
     * (Fingerprint) when fingerprint hardware is present, else `0` (None).
     */
    @JvmStatic
    fun biometricType(): Int {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return 0
            val pm = context.packageManager
            when {
                pm.hasSystemFeature("android.hardware.biometrics.face") -> 2
                pm.hasSystemFeature("android.hardware.fingerprint") -> 1
                else -> 0
            }
        } catch (e: Throwable) {
            Log.w(TAG, "biometricType failed: ${e.message}", e)
            0
        }
    }

    /**
     * Show the system biometric prompt and wait for the answer.
     *
     * @return true only if the user was recognised. Blocks, so not on the UI thread.
     */
    @JvmStatic
    fun authenticate(reason: String): Boolean {
        return try {
            SentinelBiometricHelper.authenticate(reason)
        } catch (e: Throwable) {
            Log.w(TAG, "authenticate failed: ${e.message}", e)
            false
        }
    }
}
