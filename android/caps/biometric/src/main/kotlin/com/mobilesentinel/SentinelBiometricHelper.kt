package com.mobilesentinel

import android.app.Activity
import android.util.Log
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity

/**
 * Kotlin helper for biometric authentication.
 * BiometricPrompt requires a FragmentActivity and must run on the main thread.
 */
object SentinelBiometricHelper {

    private const val TAG = "MobileSentinel.Biometric"

    /**
 * Shows the BiometricPrompt to authenticate the user.
 * Must be called with a FragmentActivity context.
 * Outcome is logged; there is no result callback into Rust — the
 * universal-SDK biometric path treats a launched prompt as success
 * and relies on the consumer's own UI flow for the final decision.
 */
    @JvmStatic
    fun authenticate(activity: Activity, reason: String) {
        if (activity !is FragmentActivity) {
            Log.e(TAG, "Activity is not a FragmentActivity, cannot show BiometricPrompt")
            return
        }

        val executor = ContextCompat.getMainExecutor(activity)

        activity.runOnUiThread {
            try {
                val callback = object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                        super.onAuthenticationSucceeded(result)
                        Log.i(TAG, "Biometric authentication succeeded")
                    }

                    override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                        super.onAuthenticationError(errorCode, errString)
                        Log.w(TAG, "Biometric authentication error: $errString (code=$errorCode)")
                    }

                    override fun onAuthenticationFailed() {
                        super.onAuthenticationFailed()
                        Log.w(TAG, "Biometric authentication failed (not recognized)")
 // Prompt stays open for retry.
                    }
                }

                val prompt = BiometricPrompt(activity, executor, callback)

                val promptInfo = BiometricPrompt.PromptInfo.Builder()
                    .setTitle("Authentication Required")
                    .setSubtitle(reason)
                    .setNegativeButtonText("Cancel")
                    .build()

                prompt.authenticate(promptInfo)
                Log.i(TAG, "BiometricPrompt shown")
            } catch (e: Exception) {
                Log.e(TAG, "Failed to show BiometricPrompt", e)
            }
        }
    }
}
