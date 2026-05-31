package com.mobilesentinel

import android.app.Activity
import android.content.Context
import android.os.Handler
import android.os.Looper
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
 *
 * Context is obtained from [SentinelPrimitives.getAppContext], which returns
 * the *application* context — never an Activity. Because `BiometricPrompt`
 * requires a `FragmentActivity`, [authenticate] can only launch when that
 * context is itself an Activity; otherwise it logs and returns false, leaving
 * the host to call [SentinelBiometricHelper.authenticate] with a real Activity.
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
     * Launch the system biometric prompt.
     *
     * `BiometricPrompt` requires a `FragmentActivity` and must run on the UI
     * thread, so this delegates to [SentinelBiometricHelper.authenticate],
     * posting the launch to the main looper. Only the *application* context is
     * available here, so the prompt can be shown only when that context is in
     * fact an Activity. When it is not, no Activity can be obtained from app
     * context, so this logs and returns false — the host must call
     * [SentinelBiometricHelper.authenticate] directly with a live Activity.
     *
     * @return true if the prompt launch was dispatched, false otherwise.
     */
    @JvmStatic
    fun authenticate(reason: String): Boolean {
        return try {
            val context: Context = SentinelPrimitives.getAppContext() ?: return false
            val activity = context as? Activity
            if (activity == null) {
                Log.w(
                    TAG,
                    "authenticate requires an Activity that cannot be obtained from app " +
                        "context; the host must call SentinelBiometricHelper.authenticate " +
                        "directly with a FragmentActivity",
                )
                return false
            }
            Handler(Looper.getMainLooper()).post {
                try {
                    SentinelBiometricHelper.authenticate(activity, reason)
                } catch (e: Throwable) {
                    Log.w(TAG, "authenticate dispatch failed: ${e.message}", e)
                }
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "authenticate failed: ${e.message}", e)
            false
        }
    }
}
