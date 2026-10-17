package com.mobilesentinel

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * What a notification tap opens. It has no screen: it remembers what the user
 * wanted ("open this chat"), brings the app's own activity to the front, and
 * finishes. The app then asks for the request with `takeOpenRequest`.
 */
class SentinelOpenActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        SentinelBackground.setOpenRequest(intent?.getStringExtra("open"))
        packageManager.getLaunchIntentForPackage(packageName)?.let {
            it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            startActivity(it)
        }
        finish()
    }
}
