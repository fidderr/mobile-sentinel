package com.mobilesentinel

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/** Starts the background service again after a reboot or an app update, if it was on. */
class SentinelBackgroundBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        val prefs = context.getSharedPreferences("sentinel_background", Context.MODE_PRIVATE)
        if (!prefs.getBoolean("enabled", false)) return
        Log.i("MobileSentinel.Boot", "starting the background service after ${intent?.action}")
        SentinelBackground.startService(context.applicationContext)
    }
}
