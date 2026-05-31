package com.mobilesentinel

import android.app.Service
import android.content.Intent
import android.os.IBinder
import android.util.Log

/**
 * Invisible background service that runs in MAIN process on boot.
 *
 * Its only job: ensure the MAIN process starts so Rust's startup runs and
 * re-arms all alarms. Then it stops itself. The user never sees anything —
 * no activity, no notification.
 *
 * Started by [SentinelBootReceiver] after BOOT_COMPLETED.
 */
class SentinelBootService : Service() {

    companion object {
        private const val TAG = "MobileSentinel.BootSvc"
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i(TAG, "Boot service started — Rust init already ran via Application.onCreate")
        // By the time we get here, Application.onCreate already fired
        // (which runs SentinelCoreInitializer ContentProvider → loads
        // the native .so). Rust's startup re-arms all alarms.
        // We just notify Rust that boot happened (for any extra recovery)
        // then stop ourselves.
        try {
            SentinelBridge.onBootCompleted()
        } catch (e: Throwable) {
            Log.w(TAG, "onBootCompleted callback failed: ${e.message}")
        }
        stopSelf()
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null
}
