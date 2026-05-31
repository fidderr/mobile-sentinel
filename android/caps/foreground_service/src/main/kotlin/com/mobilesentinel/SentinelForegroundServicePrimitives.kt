package com.mobilesentinel

import android.content.Intent
import android.os.Build
import android.util.Log

/**
 * JNI entry points for the alarm foreground service. Rust (the firing sink,
 * gated by `foreground-service`) calls these to start/stop the FGS. Thin
 * command executors — they only marshal extras onto a service Intent.
 */
object SentinelForegroundServicePrimitives {
    private const val TAG = "MobileSentinel.FgsPrim"

    @JvmStatic
    fun startForegroundService(
        channelId: String,
        channelName: String,
        title: String,
        body: String,
        importance: Int,
        bypassDnd: Boolean,
        activityFqcn: String?,
        fullScreen: Boolean,
    ): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        val intent = Intent(c, SentinelForegroundService::class.java).apply {
            action = SentinelForegroundService.ACTION_FGS_START
            putExtra(SentinelForegroundService.EXTRA_CHANNEL_ID, channelId)
            putExtra(SentinelForegroundService.EXTRA_CHANNEL_NAME, channelName)
            putExtra(SentinelForegroundService.EXTRA_TITLE, title)
            putExtra(SentinelForegroundService.EXTRA_BODY, body)
            putExtra(SentinelForegroundService.EXTRA_IMPORTANCE, importance)
            putExtra(SentinelForegroundService.EXTRA_BYPASS_DND, bypassDnd)
            putExtra(SentinelForegroundService.EXTRA_FULL_SCREEN, fullScreen)
            if (activityFqcn != null) {
                putExtra(SentinelForegroundService.EXTRA_ACTIVITY_FQCN, activityFqcn)
            }
        }
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                c.startForegroundService(intent)
            } else {
                @Suppress("DEPRECATION")
                c.startService(intent)
            }
            true
        } catch (e: Throwable) {
            Log.e(TAG, "startForegroundService: ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun stopForegroundService(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val intent = Intent(c, SentinelForegroundService::class.java).apply {
                action = SentinelForegroundService.ACTION_FGS_STOP
            }
            c.startService(intent)
            true
        } catch (e: Throwable) {
            Log.e(TAG, "stopForegroundService: ${e.message}", e)
            false
        }
    }
}
