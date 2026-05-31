package com.mobilesentinel

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Log

/**
 * Firing-vibration primitives — a sustained, looping vibration waveform while
 * an alarm fires.
 *
 * Distinct from the one-shot [SentinelHapticPrimitives] capability (UI ticks):
 * this owns a *repeating* alarm buzz that runs until [stopVibration] is called
 * on dismiss/snooze/pause. Rust (the firing sink, gated by the
 * `firing-vibration` Cargo feature) decides the pattern and when to start/stop;
 * this object only drives the [Vibrator]. Every method is defensive: it logs
 * and returns a safe fallback on failure.
 */
object SentinelFiringVibrationPrimitives {
    private const val TAG = "MobileSentinel.FiringVib"

    /**
     * Start a repeating vibration. [pattern] is the standard Android waveform
     * (alternating wait/vibrate millisecond values); it repeats from index 0
     * until [stopVibration]. A null/empty pattern is treated as "no vibration"
     * and returns false.
     */
    @JvmStatic
    fun startVibration(pattern: LongArray): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        if (pattern.isEmpty()) return false
        return try {
            val vibrator = vibrator(context) ?: return false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                // repeat = 0 → loop the whole waveform from the start.
                val effect = VibrationEffect.createWaveform(pattern, 0)
                vibrator.vibrate(effect)
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(pattern, 0)
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "startVibration(${pattern.size} entries): ${e.message}", e)
            false
        }
    }

    /** Cancel the looping firing vibration. Idempotent. */
    @JvmStatic
    fun stopVibration(): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            vibrator(context)?.cancel()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "stopVibration: ${e.message}", e)
            false
        }
    }

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
