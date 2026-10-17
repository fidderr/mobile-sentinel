package com.mobilesentinel

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log

/**
 * The quiet foreground service. Its only job is to exist: while it runs, the
 * app's process is not cached or killed, so the app's own network connection
 * (started through `SentinelBackground.startNative`) stays open and messages
 * arrive at once.
 *
 * START_STICKY so the system restarts it after killing the process. The
 * notification is the minimum Android allows (lowest importance: no status-bar
 * icon, no sound; it only shows when the shade is pulled down).
 */
class SentinelBackgroundService : Service() {

    companion object {
        private const val TAG = "MobileSentinel.BgSvc"
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        goForeground()
        startCore()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // After a system restart the intent may be null; either way we have to
        // call startForeground again promptly.
        goForeground()
        startCore()
        return START_STICKY
    }

    private fun goForeground() {
        try {
            val n = SentinelBackground.serviceNotification(this)
            if (Build.VERSION.SDK_INT >= 34) {
                startForeground(
                    SentinelBackground.SERVICE_NOTIFICATION_ID,
                    n,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_REMOTE_MESSAGING,
                )
            } else {
                startForeground(SentinelBackground.SERVICE_NOTIFICATION_ID, n)
            }
        } catch (e: Throwable) {
            Log.e(TAG, "startForeground failed: ${e.message}", e)
        }
    }

    private fun startCore() {
        val app = applicationContext
        Thread({ SentinelBackground.startNative(app) }, "sentinel-bg-start").start()
    }
}
