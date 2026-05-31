package com.mobilesentinel

import android.util.Log

/**
 * JNI entry points for kiosk mode. Rust (the firing sink, gated by the `kiosk`
 * Cargo feature) calls these to engage/release the activity lock. Thin
 * delegators to [SentinelKioskController] — all policy comes from Rust.
 */
object SentinelKioskPrimitives {
    private const val TAG = "MobileSentinel.KioskPrim"

    @JvmStatic
    fun enableKioskMode(
        activityFqcn: String,
        blockHome: Boolean,
        blockBack: Boolean,
        blockRecents: Boolean,
        relaunchDebounceMs: Int,
        hideStatusBar: Boolean,
        hideNavBar: Boolean,
    ): Boolean {
        return try {
            SentinelKioskController.enable(
                activityFqcn,
                blockHome = blockHome,
                blockBack = blockBack,
                blockRecents = blockRecents,
                relaunchDebounceMs = relaunchDebounceMs.toLong(),
                hideStatusBar = hideStatusBar,
                hideNavBar = hideNavBar,
            )
            true
        } catch (e: Throwable) {
            Log.e(TAG, "enableKioskMode: ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun disableKioskMode(): Boolean {
        return try {
            SentinelKioskController.disable()
            true
        } catch (e: Throwable) {
            Log.e(TAG, "disableKioskMode: ${e.message}", e)
            false
        }
    }
}
