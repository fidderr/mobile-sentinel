package com.mobilesentinel

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Log

/**
 * Haptic (vibration) platform primitives for the universal SDK.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around the Android [Vibrator] API — no orchestration logic, no state
 * machines, no Recipe-specific behavior. Decisions about *when* and *how
 * long* to vibrate live in Rust; Kotlin only executes the API call.
 *
 * Mirrors the haptic primitive surface (Vibrator / VibrationEffect
 * usage and API-level gating) and is reachable as `@JvmStatic` entry points.
 *
 * Threading: every method is `@JvmStatic` and stateless. The application
 * context is fetched on demand from [SentinelPrimitives.getAppContext]; JNI
 * callers are responsible for thread attachment.
 *
 * Contract: every method is total — it never throws. Failures are logged
 * via [Log] under [TAG] and surfaced as `false`.
 */
object SentinelHapticPrimitives {
    private const val TAG = "MobileSentinel.Haptic"

    /**
     * Vibrate once for [durationMs] milliseconds.
     *
     * API 26+ uses [VibrationEffect.createOneShot] with
     * [VibrationEffect.DEFAULT_AMPLITUDE]; older devices use the deprecated
     * `Vibrator.vibrate(long)` path.
     *
     * @return true if the call was issued, false on any failure.
     */
    @JvmStatic
    fun vibrate(durationMs: Long): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val vibrator = vibrator(context) ?: return false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val effect = VibrationEffect.createOneShot(durationMs, VibrationEffect.DEFAULT_AMPLITUDE)
                vibrator.vibrate(effect)
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(durationMs)
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "vibrate($durationMs): ${e.message}", e)
            false
        }
    }

    /**
     * Play a vibration waveform.
     *
     * [pattern] is the standard Android pattern: alternating wait/vibrate
     * millisecond values (`[wait, vibrate, wait, vibrate, ...]`). No repeat
     * (`-1`).
     *
     * API 26+ uses [VibrationEffect.createWaveform]; older devices use the
     * deprecated `Vibrator.vibrate(long[], int)` path.
     *
     * @return true if the call was issued, false on any failure.
     */
    @JvmStatic
    fun vibratePattern(pattern: LongArray): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val vibrator = vibrator(context) ?: return false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val effect = VibrationEffect.createWaveform(pattern, -1)
                vibrator.vibrate(effect)
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(pattern, -1)
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "vibratePattern(${pattern.size} entries): ${e.message}", e)
            false
        }
    }

    /**
     * Cancel any ongoing vibration.
     *
     * @return true if the cancel was issued, false on any failure.
     */
    @JvmStatic
    fun cancel(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val vibrator = vibrator(context) ?: return false
            vibrator.cancel()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "cancel: ${e.message}", e)
            false
        }
    }

    /**
     * Report whether the device has vibration hardware.
     *
     * @return true if a vibrator is present, false on any failure or when
     *   no vibrator is available.
     */
    @JvmStatic
    fun hasVibrator(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val vibrator = vibrator(context) ?: return false
            vibrator.hasVibrator()
        } catch (e: Throwable) {
            Log.w(TAG, "hasVibrator: ${e.message}", e)
            false
        }
    }

    /**
     * Resolve the [Vibrator] for [context].
     *
     * On API 31+ ([Build.VERSION_CODES.S]) the [VibratorManager] is the
     * supported entry point and its `defaultVibrator` is returned. On older
     * devices the deprecated `VIBRATOR_SERVICE` lookup is used.
     *
     * @return the resolved [Vibrator], or null when unavailable.
     */
    private fun vibrator(context: Context): Vibrator? {
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                val manager = context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager
                manager?.defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
            }
        } catch (e: Throwable) {
            Log.w(TAG, "vibrator(): ${e.message}", e)
            null
        }
    }
}
