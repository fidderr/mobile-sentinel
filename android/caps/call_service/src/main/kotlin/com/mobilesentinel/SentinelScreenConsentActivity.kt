package com.mobilesentinel

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.media.projection.MediaProjectionManager
import android.os.Bundle
import android.util.Log

/**
 * Nothing to see: only there to ask Android's own question about showing the screen, and to pass on
 * the answer. The answer is good for one session.
 */
class SentinelScreenConsentActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return
        try {
            val manager = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
            startActivityForResult(manager.createScreenCaptureIntent(), REQUEST)
        } catch (e: Throwable) {
            Log.e(TAG, "cannot ask: ${e.message}", e)
            SentinelCallPrimitives.nativeOnScreen(false, 0, 0)
            finish()
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQUEST) {
            if (resultCode == RESULT_OK && data != null) {
                // The service has to be of that type before the answer can be used. This activity is
                // in front, so the app may do that now.
                startForegroundService(
                    Intent(this, SentinelCallService::class.java)
                        .setAction(SentinelCallService.ACTION_SCREEN)
                        .putExtra(SentinelCallService.EXTRA_RESULT, resultCode)
                        .putExtra(SentinelCallService.EXTRA_DATA, data),
                )
            } else {
                SentinelCallPrimitives.nativeOnScreen(false, 0, 0)
            }
        }
        finish()
    }

    private companion object {
        const val TAG = "MobileSentinel.Call"
        const val REQUEST = 7301
    }
}
