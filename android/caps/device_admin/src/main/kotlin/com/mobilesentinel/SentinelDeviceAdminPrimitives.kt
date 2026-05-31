package com.mobilesentinel

import android.app.admin.DevicePolicyManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Device-admin platform primitives.
 *
 * Thin Kotlin wrappers around [DevicePolicyManager] for activating,
 * inspecting, and relinquishing device-administrator privileges bound to
 * the SDK's [SentinelAdminReceiver]. All orchestration lives in Rust; these
 * methods just execute the Android API calls.
 *
 * Every method is `@JvmStatic`, defensive (try/catch, never throws), and
 * returns `false` on any failure — including a missing application context.
 */
object SentinelDeviceAdminPrimitives {
    private const val TAG = "MobileSentinel.DeviceAdmin"

    /**
     * Short explanation shown on the system add-admin screen during
     * [requestActivation]. Kept generic — mobile-sentinel is consumer-agnostic.
     */
    private const val ADD_EXPLANATION =
        "Device admin lets the app stay active and protected while an alarm is firing."

    /**
     * Returns `true` when [SentinelAdminReceiver] is currently an active
     * device administrator, `false` otherwise (or on any error).
     */
    @JvmStatic
    fun isActive(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val dpm = context.getSystemService(Context.DEVICE_POLICY_SERVICE) as DevicePolicyManager
            dpm.isAdminActive(adminComponent(context))
        } catch (e: Throwable) {
            Log.w(TAG, "isActive failed: ${e.message}", e)
            false
        }
    }

    /**
     * Launches the system "add device admin" screen for [SentinelAdminReceiver].
     * Returns `true` if the screen was launched, `false` on any error.
     *
     * This only starts the request; the user must confirm on the system UI.
     * Use [isActive] afterwards to confirm the result.
     */
    @JvmStatic
    fun requestActivation(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val intent = Intent(DevicePolicyManager.ACTION_ADD_DEVICE_ADMIN).apply {
                putExtra(DevicePolicyManager.EXTRA_DEVICE_ADMIN, adminComponent(context))
                putExtra(DevicePolicyManager.EXTRA_ADD_EXPLANATION, ADD_EXPLANATION)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "requestActivation failed: ${e.message}", e)
            false
        }
    }

    /**
     * Removes [SentinelAdminReceiver] as an active device administrator.
     * Returns `true` on success, `false` on any error.
     */
    @JvmStatic
    fun relinquish(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            val dpm = context.getSystemService(Context.DEVICE_POLICY_SERVICE) as DevicePolicyManager
            @Suppress("DEPRECATION")
            dpm.removeActiveAdmin(adminComponent(context))
            true
        } catch (e: Throwable) {
            Log.w(TAG, "relinquish failed: ${e.message}", e)
            false
        }
    }

    /** [ComponentName] for the SDK's device-admin receiver. */
    private fun adminComponent(context: Context): ComponentName =
        ComponentName(context, SentinelAdminReceiver::class.java)
}
