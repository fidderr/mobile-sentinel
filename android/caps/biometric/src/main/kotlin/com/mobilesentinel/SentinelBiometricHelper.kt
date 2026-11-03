package com.mobilesentinel

import android.os.Looper
import android.util.Log
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Shows the system's BiometricPrompt, over the resumed FragmentActivity, and waits
 * for its answer.
 */
object SentinelBiometricHelper {

    private const val TAG = "MobileSentinel.Biometric"

    /** Nobody answered by then: not authenticated. */
    private const val TIMEOUT_SECONDS = 120L

    /**
     * Whether the user was recognised. Blocks until they answered, so not on the UI thread.
     * False when there is no FragmentActivity on screen to show the prompt over.
     */
    @JvmStatic
    fun authenticate(reason: String): Boolean {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            Log.e(TAG, "authenticate blocks: not on the UI thread")
            return false
        }
        val activity = SentinelActivityTracker.currentResumedActivity as? FragmentActivity
        if (activity == null) {
            Log.w(TAG, "no resumed FragmentActivity to show the prompt over")
            return false
        }
        val recognised = AtomicBoolean(false)
        val answered = CountDownLatch(1)
        activity.runOnUiThread {
            try {
                val callback = object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                        recognised.set(true)
                        answered.countDown()
                    }

                    override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                        Log.w(TAG, "biometric error: $errString (code=$errorCode)")
                        answered.countDown()
                    }

                    // onAuthenticationFailed: not recognised, and the prompt stays open for
                    // another try.
                }
                val prompt = BiometricPrompt(activity, ContextCompat.getMainExecutor(activity), callback)
                prompt.authenticate(
                    BiometricPrompt.PromptInfo.Builder()
                        .setTitle(reason)
                        .setNegativeButtonText(activity.getString(android.R.string.cancel))
                        .build(),
                )
            } catch (e: Exception) {
                Log.e(TAG, "showing the prompt failed", e)
                answered.countDown()
            }
        }
        answered.await(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        return recognised.get()
    }
}
