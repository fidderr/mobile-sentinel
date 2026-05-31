package com.mobilesentinel

import android.app.admin.DeviceAdminReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Generic device admin receiver shipped with mobile-sentinel.
 * Used for dismiss prevention — when active, prevents force-stopping
 * the app during alarm firing.
 */
class SentinelAdminReceiver : DeviceAdminReceiver() {

    companion object {
        private const val TAG = "MobileSentinel.Admin"
    }

    override fun onEnabled(context: Context, intent: Intent) {
        super.onEnabled(context, intent)
        Log.i(TAG, "Device admin enabled")
    }

    override fun onDisabled(context: Context, intent: Intent) {
        super.onDisabled(context, intent)
        Log.i(TAG, "Device admin disabled")
    }

    override fun onDisableRequested(context: Context, intent: Intent): CharSequence {
        return "Disabling device admin will reduce app protection capabilities."
    }
}
