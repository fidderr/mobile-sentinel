package com.mobilesentinel

import android.app.Activity
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
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
import android.widget.ProgressBar
import android.widget.TextView

/**
 * A ring's screen for an app whose own keeps failing, if that app asks for one in its ring
 * plan (`backup_holds`): a way to end a ring that would otherwise have none. Plain views in
 * `:sentinel`, with no Rust and no WebView.
 *
 * Asks for a number of long holds, which the plan decides, so an app can make it as slow as
 * what it asks for on its own screen. Rust cannot run here, so success leaves a request it
 * honours when it next runs, and ends the ring itself.
 *
 * Not for a phone that is merely locked after a boot: that is [SentinelUnlockActivity]'s.
 */
class SentinelBackupActivity : Activity() {

    companion object {
        private const val TAG = "MobileSentinel.Backup"
        private const val STEP_MS = 50L
        private const val PROGRESS_MAX = 1000
    }

    private val handler = Handler(Looper.getMainLooper())
    private var ring: SentinelRing.Ringing? = null
    private var holdsNeeded = 0
    private var holdMs = 3_000L
    private var holdsDone = 0
    private var holdStartedAt = 0L

    /** A finished hold counts once: the finger has to come up before the next. */
    private var waitingForRelease = false

    private lateinit var title: TextView
    private lateinit var count: TextView
    private lateinit var progress: ProgressBar
    private lateinit var button: TextView

    private val holdStep = object : Runnable {
        override fun run() {
            if (holdStartedAt == 0L) return
            val held = SystemClock.uptimeMillis() - holdStartedAt
            progress.progress = (held * PROGRESS_MAX / holdMs).toInt().coerceAtMost(PROGRESS_MAX)
            if (held >= holdMs) {
                holdStartedAt = 0L
                waitingForRelease = true
                holdsDone++
                if (holdsDone >= holdsNeeded) finishRing() else showCount()
            } else {
                handler.postDelayed(this, STEP_MS)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        SentinelPrimitives.init(applicationContext)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setShowWhenLocked(true)
        setTurnScreenOn(true)
        setContentView(buildViews())
        loadRing()
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
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

        count = TextView(context).apply {
            setTextColor(Color.LTGRAY)
            textSize = 20f
            gravity = Gravity.CENTER
            setPadding(0, dp(16), 0, dp(32))
        }
        addView(count)

        progress = ProgressBar(context, null, android.R.attr.progressBarStyleHorizontal).apply {
            max = PROGRESS_MAX
        }
        addView(progress, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(12)))

        button = TextView(context).apply {
            setTextColor(Color.WHITE)
            textSize = 20f
            gravity = Gravity.CENTER
            background = GradientDrawable().apply {
                setColor(Color.DKGRAY)
                cornerRadius = dp(24).toFloat()
            }
            setOnTouchListener { view, event -> onHoldTouch(view, event) }
        }
        addView(
            button,
            LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(200)).apply {
                topMargin = dp(32)
            },
        )
    }

    private fun onHoldTouch(view: android.view.View, event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (!waitingForRelease) {
                    holdStartedAt = SystemClock.uptimeMillis()
                    handler.post(holdStep)
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                holdStartedAt = 0L
                waitingForRelease = false
                handler.removeCallbacks(holdStep)
                progress.progress = 0
                view.performClick()
            }
        }
        return true
    }

    /** Show the next ringing alarm, or leave when there is none. */
    private fun loadRing() {
        val next = SentinelRing.list(this).firstOrNull()
        if (next == null) {
            finish()
            return
        }
        ring = next
        holdsNeeded = next.plan.optInt("backup_holds", 0).coerceAtLeast(1)
        holdMs = next.plan.optLong("backup_hold_ms", 3_000L).coerceAtLeast(1L)
        holdsDone = 0
        title.text = next.plan.optString("title", "").ifEmpty { applicationInfo.loadLabel(packageManager) }
        button.text = next.plan.optString("backup_text", "").ifEmpty { "Hold" }
        showCount()
    }

    private fun showCount() {
        count.text = "$holdsDone / $holdsNeeded"
        progress.progress = 0
    }

    private fun finishRing() {
        val id = ring?.id ?: return
        Log.i(TAG, "challenge done for $id")
        SentinelActiveJobs.requestDismiss(this, id)
        SentinelRing.stop(this, id)
        loadRing()
    }

    // No way out but the challenge.
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
