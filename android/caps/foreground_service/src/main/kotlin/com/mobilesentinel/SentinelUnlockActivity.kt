package com.mobilesentinel

import android.app.Activity
import android.app.KeyguardManager
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Gravity
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.TextView
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher

/**
 * What a ring shows while the phone has not been unlocked since it started and the app's own
 * screen cannot come up yet: that one needs storage that only opens with the first unlock (any
 * screen in a WebView does). It says so, and its one button asks Android for the unlock.
 *
 * Nothing here ends the ring. That stays with the app, whose screen takes over the moment the
 * phone is unlocked. Plain views in `:sentinel`, with no Rust and no WebView; its texts come
 * from the ring plan.
 */
class SentinelUnlockActivity : Activity() {

    companion object {
        private const val TAG = "MobileSentinel.Unlock"
        private const val CHECK_MS = 1_000L

        /** Whether this screen is in front, for the ring service next to it in `:sentinel`. */
        @Volatile
        @JvmStatic
        var inFront = false
            private set

        /**
         * When this screen last started the app's own, on the clock that counts from the boot.
         * The ring service does not start it a second time right after.
         */
        @Volatile
        @JvmStatic
        var handedOverAt = 0L
            private set
    }

    private val handler = Handler(Looper.getMainLooper())
    private var ring: SentinelRing.Ringing? = null
    private var handedOver = false

    private lateinit var title: TextView
    private lateinit var text: TextView
    private lateinit var button: TextView

    /** The storage is open: the app's screen can come up now. */
    private val unlocked = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = handOver()
    }

    /** For a ring that ended without this screen, and an unlock whose news got lost. */
    private val check = object : Runnable {
        override fun run() {
            if (SentinelRing.list(this@SentinelUnlockActivity).isEmpty()) {
                finish()
                return
            }
            handOver()
            if (!handedOver) handler.postDelayed(this, CHECK_MS)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        SentinelPrimitives.init(applicationContext)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setShowWhenLocked(true)
        setTurnScreenOn(true)
        setContentView(buildViews())
        // Back leads nowhere: there is nothing behind this screen but the lock screen.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            onBackInvokedDispatcher.registerOnBackInvokedCallback(
                OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                OnBackInvokedCallback { },
            )
        }
        registerReceiver(unlocked, IntentFilter(Intent.ACTION_USER_UNLOCKED))
        load()
    }

    /** Brought to the front again while it was still there. */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        load()
    }

    override fun onResume() {
        super.onResume()
        inFront = true
        handler.removeCallbacks(check)
        handler.post(check)
    }

    override fun onPause() {
        inFront = false
        handler.removeCallbacks(check)
        super.onPause()
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        unregisterReceiver(unlocked)
        super.onDestroy()
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()

    private fun buildViews() = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER
        setBackgroundColor(Color.BLACK)
        setPadding(dp(24), dp(24), dp(24), dp(24))

        title = TextView(context).apply {
            setTextColor(Color.WHITE)
            textSize = 28f
            gravity = Gravity.CENTER
        }
        addView(title)

        text = TextView(context).apply {
            setTextColor(Color.LTGRAY)
            textSize = 20f
            gravity = Gravity.CENTER
            setPadding(0, dp(16), 0, dp(32))
        }
        addView(text)

        button = TextView(context).apply {
            setTextColor(Color.WHITE)
            textSize = 20f
            gravity = Gravity.CENTER
            background = GradientDrawable().apply {
                setColor(Color.DKGRAY)
                cornerRadius = dp(24).toFloat()
            }
            setOnClickListener { askForUnlock() }
        }
        addView(
            button,
            LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(96)).apply {
                topMargin = dp(32)
            },
        )
    }

    /** Show the ring this is about, or leave when nothing rings. */
    private fun load() {
        val next = SentinelRing.list(this).firstOrNull()
        if (next == null) {
            finish()
            return
        }
        ring = next
        title.text = next.plan.optString("title", "").ifEmpty { applicationInfo.loadLabel(packageManager) }
        text.text = next.plan.optString("unlock_text", "").ifEmpty { "Unlock your phone to continue." }
        button.text = next.plan.optString("unlock_button", "").ifEmpty { "Unlock" }
    }

    /**
     * Android's own unlock, over this screen. The ring may go quiet for it; the service brings it
     * back when the phone is unlocked, and when that takes too long.
     */
    private fun askForUnlock() {
        SentinelRing.noteActivity(this)
        handOver()
        if (handedOver) return
        val keyguard = getSystemService(KeyguardManager::class.java)
        if (keyguard == null) {
            stepAside()
            return
        }
        SentinelRing.quietForUnlock(this)
        keyguard.requestDismissKeyguard(
            this,
            object : KeyguardManager.KeyguardDismissCallback() {
                // The storage may open a moment later: then the receiver or the check hands over.
                override fun onDismissSucceeded() = handOver()

                override fun onDismissCancelled() {
                    SentinelRing.endQuiet(this@SentinelUnlockActivity)
                }

                override fun onDismissError() {
                    Log.w(TAG, "Android does not show its unlock over this screen, stepping aside")
                    stepAside()
                }
            },
        )
    }

    /** Out of the way of the lock screen itself. The ring service brings this back in time. */
    private fun stepAside() {
        moveTaskToBack(true)
    }

    /** Once the phone is unlocked: the app's own screen, in place of this one. */
    private fun handOver() {
        if (handedOver || !SentinelBoot.isUserUnlocked(this)) return
        handedOver = true
        handedOverAt = SystemClock.elapsedRealtime()
        val activity = ring?.plan?.optString("activity_fqcn", "").orEmpty()
        if (activity.isNotEmpty()) {
            Log.i(TAG, "unlocked, handing over to the app's screen")
            try {
                startActivity(
                    Intent().apply {
                        component = ComponentName(packageName, activity)
                        flags = Intent.FLAG_ACTIVITY_NEW_TASK
                    },
                )
            } catch (e: Throwable) {
                Log.w(TAG, "starting the app's screen failed: ${e.message}")
            }
        }
        finish()
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
    }

    /** Somebody is there: the ring limit starts over, as on the app's own screen. */
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) SentinelRing.noteActivity(this)
        return super.dispatchTouchEvent(event)
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean = when (event.keyCode) {
        KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN, KeyEvent.KEYCODE_VOLUME_MUTE -> true
        else -> super.dispatchKeyEvent(event)
    }
}
