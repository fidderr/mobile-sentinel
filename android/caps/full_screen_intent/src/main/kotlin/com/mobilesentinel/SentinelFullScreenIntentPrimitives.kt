package com.mobilesentinel

import android.content.Intent
import android.util.Log

/**
 * Full-screen-intent primitives — wake the lock screen and show the firing
 * activity.
 *
 * Rust calls INTO this via JNI (the firing sink, gated by the
 * `full-screen-intent` Cargo feature). Rust decides WHEN and WHICH activity;
 * this object just launches it. Defensive: logs and returns false on failure.
 */
object SentinelFullScreenIntentPrimitives {
    private const val TAG = "MobileSentinel.FSI"

    @JvmStatic
    fun showFullScreenIntent(activityFqcn: String, dismissKeyguard: Boolean): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        // It would only cover the backup screen, the one way out.
        if (SentinelRing.uiFailed(c)) return false
        // Until the first unlock Android does not start it: the ring service asks for the unlock.
        if (!SentinelBoot.screenCanStart(c, activityFqcn)) return false
        return try {
            val cls = Class.forName(activityFqcn)
            val intent = Intent(c, cls).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                if (dismissKeyguard) {
                    addFlags(Intent.FLAG_ACTIVITY_NO_USER_ACTION)
                }
            }
            c.startActivity(intent)
            true
        } catch (e: Throwable) {
            Log.e(TAG, "showFullScreenIntent: ${e.message}", e)
            false
        }
    }
}
