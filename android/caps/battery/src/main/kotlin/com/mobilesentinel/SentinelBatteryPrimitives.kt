package com.mobilesentinel

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import android.util.Log

/**
 * Battery optimization exemption primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around an Android API — no orchestration logic. Reached from Rust via
 * `primitives_ext::battery_*`.
 *
 * Every method is `@JvmStatic`, wrapped in try/catch, logs via
 * [Log.w], never throws, and returns a safe fallback. The application
 * context is obtained from [SentinelPrimitives.getAppContext]; when it is
 * null the safe fallback is returned.
 */
object SentinelBatteryPrimitives {
    private const val TAG = "MobileSentinel.Battery"

    /**
     * Returns true when the app is currently ignoring battery
     * optimizations (i.e. exempt from Doze restrictions).
     *
     * Safe fallback: false.
     */
    @JvmStatic
    fun isExempt(): Boolean {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: run {
                Log.w(TAG, "isExempt: no app context")
                return false
            }
            val pm = c.getSystemService(Context.POWER_SERVICE) as PowerManager
            pm.isIgnoringBatteryOptimizations(c.packageName)
        } catch (e: Throwable) {
            Log.w(TAG, "isExempt: ${e.message}")
            false
        }
    }

    /**
     * Launches the per-app "ignore battery optimizations" request dialog.
     * Requires the REQUEST_IGNORE_BATTERY_OPTIMIZATIONS permission in the
     * manifest.
     *
     * Returns true if the settings intent was launched.
     * Safe fallback: false.
     */
    @JvmStatic
    fun requestExemption(): Boolean {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: run {
                Log.w(TAG, "requestExemption: no app context")
                return false
            }
            @Suppress("BatteryLife")
            val intent = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
                data = Uri.parse("package:${c.packageName}")
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            c.startActivity(intent)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "requestExemption: ${e.message}")
            false
        }
    }

    /**
     * Opens the general battery-optimization settings list (all apps).
     *
     * Returns true if the settings intent was launched.
     * Safe fallback: false.
     */
    @JvmStatic
    fun openSettings(): Boolean {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: run {
                Log.w(TAG, "openSettings: no app context")
                return false
            }
            val intent = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            c.startActivity(intent)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "openSettings: ${e.message}")
            false
        }
    }
}
