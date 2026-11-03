package com.mobilesentinel

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * What clicking a link of the app's own scheme (`myapp://add/...`) opens.
 * Like [SentinelOpenActivity] it has no screen: it remembers the link as an
 * open request ("link:<uri>"), brings the app to the front and finishes. The
 * app then asks for the request with `takeOpenRequest`.
 */
class SentinelLinkActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        intent?.dataString?.let { SentinelBackground.setOpenRequest("link:$it") }
        packageManager.getLaunchIntentForPackage(packageName)?.let {
            it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            startActivity(it)
        }
        finish()
    }
}
