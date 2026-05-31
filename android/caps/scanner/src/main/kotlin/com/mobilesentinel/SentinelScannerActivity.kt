package com.mobilesentinel

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Rect
import android.graphics.drawable.GradientDrawable
import android.hardware.Camera
import android.os.Bundle
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.TextView
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import com.journeyapps.barcodescanner.BarcodeCallback
import com.journeyapps.barcodescanner.BarcodeResult
import com.journeyapps.barcodescanner.DecoratedBarcodeView
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

/**
 * Full-screen barcode / QR scanner activity using ZXing embedded.
 * Handles camera permission, the live preview, continuous + tap-to-focus, a
 * torch toggle, and a cancel control in one activity.
 *
 * ## Why this activity's window is **translucent** (critical — do not change)
 *
 * The consumer's host activity (the one rendering the UI — e.g. a WebView for
 * Dioxus/Tauri) is typically `launchMode="singleInstance"` (mobile-sentinel
 * sets that for the lock-screen / firing path). If this scanner used an
 * *opaque* full-screen theme, the host — being in a separate task — would be
 * driven to `onStop`, which tears down its render surface (EGL). A WebView host
 * does NOT reliably rebuild that surface on return, so the consumer UI comes
 * back **frozen** (no repaint, no touch).
 *
 * A **translucent** window keeps the host in `onPause` (visible-behind) instead
 * of `onStop`, so its surface is never destroyed and the UI stays live. We then
 * paint a fully opaque background + a TextureView camera preview (ZXing's
 * default), so visually this looks like an ordinary solid full-screen scanner —
 * the translucency is a lifecycle mechanism, not a visual one.
 *
 * ## Result delivery
 *
 * Writes the scanned value to `filesDir/sentinel_scan_result.txt`, which the
 * Rust side ([`crate::features::camera`]) polls for. This avoids relying on
 * `onActivityResult`, which doesn't work with NativeActivity-based hosts.
 */
class SentinelScannerActivity : Activity() {

    private companion object {
        const val TAG = "MobileSentinel.ScanAct"
        const val PERMISSION_REQUEST_CAMERA = 100
        const val RESULT_FILE = "sentinel_scan_result.txt"

        // Opaque backdrop so the translucent window reads as a solid scanner.
        const val COLOR_BACKDROP = 0xFF0E0E14.toInt()
        const val COLOR_CONTROL_BG = 0x66000000 // semi-transparent black chip
        const val COLOR_CONTROL_FG = Color.WHITE
        const val COLOR_TORCH_ON = 0xFFFFC850.toInt() // warm amber when lit

        // Tap-to-focus: half-size of the focus/metering rect in the Camera1
        // -1000..1000 coordinate space, and the touch-slop (px) under which a
        // press counts as a tap rather than a drag.
        const val FOCUS_AREA_HALF = 150
        const val TAP_SLOP_PX = 24f
    }

    private var rootLayout: FrameLayout? = null
    private var barcodeView: DecoratedBarcodeView? = null
    private var torchButton: TextView? = null
    private var scanResult: String? = null
    private var torchOn = false

    // Tap tracking for tap-to-focus (distinguish tap from drag).
    private var downX = 0f
    private var downY = 0f

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Show over the lock screen + turn the screen on, matching the host's
        // lock-screen flags so scanning works when the phone is locked.
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
            try {
                val km = getSystemService(android.app.KeyguardManager::class.java)
                km?.requestDismissKeyguard(this, null)
            } catch (e: Throwable) {
                Log.w(TAG, "requestDismissKeyguard failed: ${e.message}")
            }
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(
                android.view.WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or
                    android.view.WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON or
                    android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
            )
        }

        if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA)
            != PackageManager.PERMISSION_GRANTED
        ) {
            ActivityCompat.requestPermissions(
                this,
                arrayOf(Manifest.permission.CAMERA),
                PERMISSION_REQUEST_CAMERA
            )
        } else {
            startScanning()
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        if (requestCode == PERMISSION_REQUEST_CAMERA) {
            if (grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED) {
                startScanning()
            } else {
                Log.w(TAG, "Camera permission denied")
                finish()
            }
        }
    }

    private fun startScanning() {
        // Opaque root so the translucent window looks like a solid scanner and
        // nothing of the host behind shows through.
        val root = FrameLayout(this).apply {
            setBackgroundColor(COLOR_BACKDROP)
            layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
        }
        rootLayout = root

        // Live camera preview (ZXing defaults to a TextureView, which composites
        // correctly in the normal view hierarchy over our opaque backdrop).
        barcodeView = DecoratedBarcodeView(this).apply {
            // No prose — keep the SDK app-neutral and language-neutral. The
            // dimmed viewfinder framing communicates "aim here" on its own.
            setStatusText("")
            decodeSingle(object : BarcodeCallback {
                override fun barcodeResult(result: BarcodeResult) {
                    val content = result.text ?: ""
                    Log.i(TAG, "Scanned: ${content.take(50)}")
                    scanResult = content
                    writeResultFile(content)
                    finish()
                }
            })
        }
        root.addView(
            barcodeView,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
        )

        // Tap-to-focus on the preview area. Attached to root so the control
        // chips (which are clickable and added on top) keep their own taps;
        // anything that reaches the preview triggers a focus at that point.
        root.setOnTouchListener { _, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = ev.x
                    downY = ev.y
                    true
                }
                MotionEvent.ACTION_UP -> {
                    if (abs(ev.x - downX) <= TAP_SLOP_PX && abs(ev.y - downY) <= TAP_SLOP_PX) {
                        focusAt(ev.x, ev.y)
                    }
                    true
                }
                else -> false
            }
        }

        // Cancel control (top-start) — icon-only ✕.
        val cancel = makeControl("\u2715", COLOR_CONTROL_FG).apply {
            layoutParams = FrameLayout.LayoutParams(
                dp(48), dp(48), Gravity.TOP or Gravity.START
            ).apply { setMargins(dp(16), dp(16), 0, 0) }
            setOnClickListener {
                writeResultFile("")
                finish()
            }
        }
        root.addView(cancel)

        // Torch toggle (top-end) — icon-only.
        torchButton = makeControl("\u26A1", COLOR_CONTROL_FG).apply {
            layoutParams = FrameLayout.LayoutParams(
                dp(48), dp(48), Gravity.TOP or Gravity.END
            ).apply { setMargins(0, dp(16), dp(16), 0) }
            setOnClickListener { toggleTorch() }
        }
        root.addView(torchButton)

        setContentView(root)
        enterImmersive()
        barcodeView?.resume()
        Log.i(TAG, "Scanner started")
    }

    /** Build a circular, semi-transparent icon button with a glyph label. */
    private fun makeControl(glyph: String, fg: Int): TextView {
        return TextView(this).apply {
            text = glyph
            setTextColor(fg)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 20f)
            gravity = Gravity.CENTER
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(COLOR_CONTROL_BG)
            }
            isClickable = true
            isFocusable = true
        }
    }

    private fun toggleTorch() {
        torchOn = !torchOn
        try {
            if (torchOn) barcodeView?.setTorchOn() else barcodeView?.setTorchOff()
        } catch (e: Throwable) {
            Log.w(TAG, "torch toggle failed: ${e.message}")
        }
        torchButton?.setTextColor(if (torchOn) COLOR_TORCH_ON else COLOR_CONTROL_FG)
    }

    /**
     * Tap-to-focus: nudge the camera to focus + meter on the tapped point.
     *
     * Drives the legacy Camera1 parameters ZXing embedded uses: sets a focus
     * (and metering) area centered on the tap and switches to AUTO focus so the
     * driver refocuses there. Heavily guarded — OEM camera HALs vary, and some
     * don't support focus areas at all (we simply no-op the focus there but
     * still show the ring). A focus ring animates at the tap for feedback.
     */
    private fun focusAt(x: Float, y: Float) {
        showFocusRing(x, y)
        val preview = barcodeView?.barcodeView ?: return
        val w = preview.width
        val h = preview.height
        if (w <= 0 || h <= 0) return
        try {
            preview.changeCameraParameters { params ->
                try {
                    val area = computeFocusArea(x, y, w, h)
                    val areas = listOf(Camera.Area(area, 1000))
                    if (params.maxNumFocusAreas > 0) {
                        val modes = params.supportedFocusModes
                        if (modes != null && modes.contains(Camera.Parameters.FOCUS_MODE_AUTO)) {
                            params.focusMode = Camera.Parameters.FOCUS_MODE_AUTO
                        }
                        params.focusAreas = areas
                    }
                    if (params.maxNumMeteringAreas > 0) {
                        params.meteringAreas = areas
                    }
                } catch (e: Throwable) {
                    Log.w(TAG, "focus params failed: ${e.message}")
                }
                params
            }
        } catch (e: Throwable) {
            Log.w(TAG, "changeCameraParameters failed: ${e.message}")
        }
    }

    /**
     * Map a screen tap to a Camera1 focus `Rect` in the -1000..1000 space.
     * The scanner is portrait-locked and the back sensor is mounted landscape
     * (90° rotation), so the screen Y axis maps to the camera X axis and the
     * screen X axis maps to the (inverted) camera Y axis. Approximate by design
     * — a generous area size keeps it forgiving of per-device sensor variance.
     */
    private fun computeFocusArea(x: Float, y: Float, w: Int, h: Int): Rect {
        val cx = clampCoord((y / h) * 2000f - 1000f)
        val cy = clampCoord(-((x / w) * 2000f - 1000f))
        val left = clampCoord(cx - FOCUS_AREA_HALF).toInt()
        val top = clampCoord(cy - FOCUS_AREA_HALF).toInt()
        val right = clampCoord(cx + FOCUS_AREA_HALF).toInt()
        val bottom = clampCoord(cy + FOCUS_AREA_HALF).toInt()
        return Rect(min(left, right), min(top, bottom), max(left, right), max(top, bottom))
    }

    private fun clampCoord(v: Float): Float = max(-1000f, min(1000f, v))

    /** Animate a brief focus ring at the tapped location for feedback. */
    private fun showFocusRing(x: Float, y: Float) {
        val root = rootLayout ?: return
        val size = dp(64)
        val ring = View(this).apply {
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setStroke(dp(2), Color.WHITE)
                setColor(Color.TRANSPARENT)
            }
            layoutParams = FrameLayout.LayoutParams(size, size).apply {
                leftMargin = (x - size / 2f).toInt()
                topMargin = (y - size / 2f).toInt()
            }
            alpha = 0f
            scaleX = 1.5f
            scaleY = 1.5f
        }
        root.addView(ring)
        ring.animate()
            .alpha(1f).scaleX(1f).scaleY(1f)
            .setDuration(160)
            .withEndAction {
                ring.animate()
                    .alpha(0f)
                    .setStartDelay(420)
                    .setDuration(200)
                    .withEndAction { root.removeView(ring) }
                    .start()
            }
            .start()
    }

    /** Hide the system bars for a clean full-screen scan surface. */
    private fun enterImmersive() {
        @Suppress("DEPRECATION")
        window.decorView.systemUiVisibility = (
            View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                or View.SYSTEM_UI_FLAG_FULLSCREEN
                or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                or View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
            )
    }

    private fun dp(value: Int): Int =
        (value * resources.displayMetrics.density).toInt()

    private fun writeResultFile(content: String) {
        try {
            java.io.File(filesDir, RESULT_FILE).writeText(content)
            Log.i(TAG, "Result written to file")
        } catch (e: Throwable) {
            Log.e(TAG, "Failed to write scan result file: ${e.message}", e)
        }
    }

    /**
     * Bring the host (launcher) activity back to the front after the scanner
     * finishes. Even with a translucent window the host can need an explicit
     * nudge to re-resume; re-launching via its package launch intent with
     * `REORDER_TO_FRONT | SINGLE_TOP` delivers through `onNewIntent` (not a
     * recreate), restoring focus. Capability-agnostic — resolves whatever
     * launcher activity the consumer ships.
     */
    private fun returnToHost() {
        try {
            val launch = packageManager.getLaunchIntentForPackage(packageName)
            if (launch != null) {
                launch.addFlags(
                    Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or
                        Intent.FLAG_ACTIVITY_SINGLE_TOP
                )
                startActivity(launch)
                Log.i(TAG, "Returned to host activity")
            } else {
                Log.w(TAG, "No launch intent for $packageName; cannot return to host")
            }
        } catch (e: Throwable) {
            Log.w(TAG, "returnToHost failed: ${e.message}")
        }
    }

    override fun finish() {
        returnToHost()
        super.finish()
    }

    override fun onResume() {
        super.onResume()
        enterImmersive()
        barcodeView?.resume()
    }

    override fun onPause() {
        super.onPause()
        barcodeView?.pause()
    }

    override fun onDestroy() {
        super.onDestroy()
        barcodeView?.pause()
        // Safety net: if the activity is destroyed without a scan, write an
        // empty result so the Rust poll loop doesn't hang to its timeout.
        if (scanResult == null) {
            writeResultFile("")
        }
    }

    @Deprecated("Uses legacy onBackPressed for broad SDK support; behavior is intentional.")
    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        // User cancelled — write empty file so the poll returns promptly.
        writeResultFile("")
        super.onBackPressed()
    }
}
