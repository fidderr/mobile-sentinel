package com.mobilesentinel.smoke

import android.app.Activity
import android.os.Bundle
import android.widget.TextView
import com.mobilesentinel.SentinelCompat

/** What the smoke test reads of the screens it opens. */
object SmokeScreens {
    @Volatile
    var resumed: Activity? = null

    /** How often a screen came to the front, so a test sees one that was covered again at once. */
    val resumes = java.util.concurrent.atomic.AtomicInteger()
}

open class SmokeBaseActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(TextView(this).apply { text = "smoke" })
    }

    override fun onResume() {
        super.onResume()
        SmokeScreens.resumed = this
        SmokeScreens.resumes.incrementAndGet()
    }

    override fun onPause() {
        if (SmokeScreens.resumed === this) SmokeScreens.resumed = null
        super.onPause()
    }
}

/** Asks to show over the lock screen at run time, like the kiosk lock does for an app's own screen. */
class SmokeActivity : SmokeBaseActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        SentinelCompat.showOverLockScreen(this)
        super.onCreate(savedInstanceState)
    }
}

/** A ring's screen as an app declares it: show over the lock screen and turn the screen on, in the manifest. */
class SmokeRingActivity : SmokeBaseActivity()
