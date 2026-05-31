package com.mobilesentinel

import android.content.Context
import android.os.PowerManager
import android.util.Log
import java.util.concurrent.ConcurrentHashMap

/**
 * Wake-lock primitives — keep the CPU awake while an alarm fires.
 *
 * Rust calls INTO these via JNI (the firing sink, gated by the `wake-lock`
 * Cargo feature). No orchestration, no state machine — Rust decides when to
 * acquire/release; this object just holds the `PARTIAL_WAKE_LOCK` handles by
 * tag. Every method is defensive: logs and returns a safe fallback on failure.
 */
object SentinelWakeLockPrimitives {
    private const val TAG = "MobileSentinel.WakeLock"

    /** Wake locks held by tag — released by [releaseWakeLock]. */
    private val wakeLocks = ConcurrentHashMap<String, PowerManager.WakeLock>()

    @JvmStatic
    fun acquireWakeLock(tag: String, timeoutMs: Long): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val pm = c.getSystemService(Context.POWER_SERVICE) as PowerManager
            val wl = pm.newWakeLock(
                PowerManager.PARTIAL_WAKE_LOCK,
                "MobileSentinel:$tag",
            )
            wl.setReferenceCounted(false)
            if (timeoutMs > 0) wl.acquire(timeoutMs) else wl.acquire()
            wakeLocks[tag] = wl
            true
        } catch (e: Throwable) {
            Log.e(TAG, "acquireWakeLock($tag): ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun releaseWakeLock(tag: String): Boolean {
        return try {
            wakeLocks.remove(tag)?.let { wl ->
                if (wl.isHeld) wl.release()
            }
            true
        } catch (e: Throwable) {
            Log.e(TAG, "releaseWakeLock($tag): ${e.message}", e)
            false
        }
    }
}
