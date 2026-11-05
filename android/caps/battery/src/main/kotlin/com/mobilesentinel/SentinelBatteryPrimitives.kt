package com.mobilesentinel

import android.app.ActivityManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import android.util.Log

/**
 * Battery optimization exemption primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around an Android API — no orchestration logic. Reached from Rust via
 * `crate::features::battery`.
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
     * Returns false when someone restricted this app's background use. Safe fallback: true, since
     * a warning that may be false teaches people to ignore warnings.
     */
    @JvmStatic
    fun backgroundAllowed(): Boolean {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: return true
            !c.getSystemService(ActivityManager::class.java).isBackgroundRestricted
        } catch (e: Throwable) {
            Log.w(TAG, "backgroundAllowed: ${e.message}")
            true
        }
    }

    /**
     * Returns false when the system takes this app's permissions away after months without use.
     * Safe fallback: true.
     */
    @JvmStatic
    fun keptWhenUnused(): Boolean {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: return true
            c.packageManager.isAutoRevokeWhitelisted
        } catch (e: Throwable) {
            Log.w(TAG, "keptWhenUnused: ${e.message}")
            true
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

    @Volatile
    private var watching = false

    private const val LOW_PERCENT = 15

    /**
     * What the battery is up to, as "charging,powerSave,low" with 0 or 1 for each. A phone that
     * cannot say counts as on the charger: nothing is held back for no reason.
     */
    @JvmStatic
    fun powerState(): String {
        return try {
            val c: Context = SentinelPrimitives.getAppContext() ?: return "1,0,0"
            val sticky = c.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
            val plugged = sticky?.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) ?: 0
            val level = sticky?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
            val scale = sticky?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
            val charging = plugged != 0
            val low = level >= 0 && scale > 0 && level * 100 / scale <= LOW_PERCENT
            val pm = c.getSystemService(Context.POWER_SERVICE) as PowerManager
            val flag = { on: Boolean -> if (on) "1" else "0" }
            "${flag(charging)},${flag(pm.isPowerSaveMode)},${flag(low)}"
        } catch (e: Throwable) {
            Log.w(TAG, "powerState: ${e.message}")
            "1,0,0"
        }
    }

    /** From now on [nativeOnPowerChange] hears when any of that may have changed. */
    @JvmStatic
    fun watchPower(): Boolean {
        return try {
            synchronized(this) {
                if (watching) return true
                val c: Context = SentinelPrimitives.getAppContext() ?: return false
                val filter = IntentFilter().apply {
                    addAction(Intent.ACTION_POWER_CONNECTED)
                    addAction(Intent.ACTION_POWER_DISCONNECTED)
                    addAction(Intent.ACTION_BATTERY_LOW)
                    addAction(Intent.ACTION_BATTERY_OKAY)
                    addAction(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED)
                }
                val receiver = object : BroadcastReceiver() {
                    override fun onReceive(context: Context?, intent: Intent?) {
                        try {
                            nativeOnPowerChange()
                        } catch (e: UnsatisfiedLinkError) {
                            Log.w(TAG, "nativeOnPowerChange not linked: ${e.message}")
                        }
                    }
                }
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    c.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
                } else {
                    c.registerReceiver(receiver, filter)
                }
                watching = true
                true
            }
        } catch (e: Throwable) {
            Log.w(TAG, "watchPower: ${e.message}")
            false
        }
    }

    @JvmStatic
    private external fun nativeOnPowerChange()
}
