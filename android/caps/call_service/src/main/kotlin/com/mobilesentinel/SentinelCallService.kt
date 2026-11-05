package com.mobilesentinel

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.util.DisplayMetrics
import android.util.Log
import android.view.Display
import android.view.Surface
import androidx.core.app.NotificationCompat

/**
 * A call that goes on while the app is not in front. Android cuts off the microphone and camera of
 * an app in the background unless a service of that type runs, and showing the screen needs one too.
 *
 * Frames go straight from Android's encoder to Rust: while the screen is shown the app is usually
 * behind another one, so nothing in its page may be needed for it.
 */
class SentinelCallService : Service() {

    private var title = ""
    private var text = ""
    private var camera = false
    private var projection: MediaProjection? = null
    private var display: VirtualDisplay? = null
    private var encoder: MediaCodec? = null
    private var surface: Surface? = null
    private var worker: HandlerThread? = null
    private var handler: Handler? = null
    @Volatile private var showing = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        current = this
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_CALL -> {
                title = intent.getStringExtra(EXTRA_TITLE) ?: title
                text = intent.getStringExtra(EXTRA_TEXT) ?: text
                camera = intent.getBooleanExtra(EXTRA_CAMERA, false)
                goForeground(showing)
            }
            ACTION_SCREEN -> {
                val data = intent.parcelable()
                if (data == null) {
                    SentinelCallPrimitives.nativeOnScreen(false, 0, 0)
                } else {
                    goForeground(true)
                    startScreen(intent.getIntExtra(EXTRA_RESULT, 0), data)
                }
            }
            ACTION_STOP_SCREEN -> {
                stopScreen()
                goForeground(false)
            }
            else -> goForeground(false)
        }
        return START_NOT_STICKY
    }

    private fun Intent.parcelable(): Intent? = getParcelableExtra(EXTRA_DATA, Intent::class.java)

    private fun goForeground(withScreen: Boolean) {
        var types = ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        if (camera) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA
        if (withScreen) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION
        try {
            startForeground(NOTIFICATION_ID, notification(withScreen), types)
        } catch (e: Throwable) {
            // Without the permission for one of the types: at least the rest.
            Log.e(TAG, "startForeground failed: ${e.message}", e)
        }
    }

    private fun notification(withScreen: Boolean): Notification {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val label = applicationInfo.loadLabel(packageManager).toString()
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, label, NotificationManager.IMPORTANCE_LOW).apply { setShowBadge(false) },
        )
        val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        val builder = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.stat_sys_phone_call)
            .setContentTitle(title.ifEmpty { label })
            .setContentText(text)
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setContentIntent(open)
        if (withScreen) {
            val stop = PendingIntent.getService(
                this,
                1,
                Intent(this, SentinelCallService::class.java).setAction(ACTION_STOP_SCREEN),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            builder.addAction(0, stopLabel, stop)
        }
        return builder.build()
    }

    // ---- showing the screen ----

    private fun startScreen(result: Int, data: Intent) {
        try {
            stopScreen()
            val thread = HandlerThread("sentinel-screen").also { it.start() }
            worker = thread
            val h = Handler(thread.looper)
            handler = h
            val manager = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
            val p = manager.getMediaProjection(result, data) ?: throw IllegalStateException("no projection")
            projection = p
            p.registerCallback(
                object : MediaProjection.Callback() {
                    // The chip in the status bar, the screen being locked, another app taking over.
                    override fun onStop() {
                        h.post { stopScreen() }
                    }

                    // A phone turned, one app chosen, a window resized.
                    override fun onCapturedContentResize(width: Int, height: Int) {
                        h.post { resize(width, height) }
                    }
                },
                h,
            )
            val metrics = realMetrics()
            val (w, h2) = fit(metrics.widthPixels, metrics.heightPixels)
            val s = startEncoder(w, h2)
            display = p.createVirtualDisplay(
                "sentinel-screen",
                w,
                h2,
                metrics.densityDpi,
                DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
                s,
                null,
                h,
            )
            showing = true
            SentinelCallPrimitives.nativeOnScreen(true, w, h2)
        } catch (e: Throwable) {
            Log.e(TAG, "showing the screen failed: ${e.message}", e)
            stopScreen()
            SentinelCallPrimitives.nativeOnScreen(false, 0, 0)
        }
    }

    @Suppress("DEPRECATION")
    private fun realMetrics(): DisplayMetrics {
        val metrics = DisplayMetrics()
        (getSystemService(Context.DISPLAY_SERVICE) as DisplayManager)
            .getDisplay(Display.DEFAULT_DISPLAY)
            .getRealMetrics(metrics)
        return metrics
    }

    /** Not bigger than it has to be, and in the steps encoders ask for. */
    private fun fit(width: Int, height: Int): Pair<Int, Int> {
        val longest = maxOf(width, height)
        val scale = if (longest > MAX_SIDE) MAX_SIDE.toFloat() / longest else 1f
        val round = { v: Int -> maxOf(16, (v * scale).toInt() / 16 * 16) }
        return round(width) to round(height)
    }

    private fun startEncoder(width: Int, height: Int): Surface {
        var last: Throwable? = null
        // H.264 first: every phone has it, usually in hardware. Every viewer can read both.
        for ((mime, codec) in listOf(MediaFormat.MIMETYPE_VIDEO_AVC to 2, MediaFormat.MIMETYPE_VIDEO_VP8 to 0)) {
            try {
                val format = MediaFormat.createVideoFormat(mime, width, height).apply {
                    setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
                    setInteger(MediaFormat.KEY_BIT_RATE, BITRATE)
                    setInteger(MediaFormat.KEY_FRAME_RATE, FPS)
                    setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, WHOLE_PICTURE_EVERY)
                    // A screen that stands still gives no frames: someone new must still get one.
                    setLong(MediaFormat.KEY_REPEAT_PREVIOUS_FRAME_AFTER, 1_000_000L)
                    setFloat(MediaFormat.KEY_MAX_FPS_TO_ENCODER, FPS.toFloat())
                }
                val c = MediaCodec.createEncoderByType(mime)
                c.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                val s = c.createInputSurface()
                c.setCallback(Drain(codec), handler)
                c.start()
                encoder = c
                surface = s
                return s
            } catch (e: Throwable) {
                Log.w(TAG, "no encoder for $mime: ${e.message}")
                last = e
            }
        }
        throw last ?: IllegalStateException("no encoder")
    }

    private inner class Drain(private val codec: Int) : MediaCodec.Callback() {
        /** H.264 says how to read it once, and whoever comes later needs it with a whole picture. */
        private var config: ByteArray? = null

        override fun onInputBufferAvailable(c: MediaCodec, index: Int) = Unit

        override fun onOutputBufferAvailable(c: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            try {
                val buffer = c.getOutputBuffer(index)
                if (buffer != null && info.size > 0) {
                    buffer.position(info.offset)
                    buffer.limit(info.offset + info.size)
                    val bytes = ByteArray(info.size)
                    buffer.get(bytes)
                    if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                        config = bytes
                    } else {
                        val key = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
                        val whole = config
                        val out = if (key && whole != null) whole + bytes else bytes
                        SentinelCallPrimitives.nativeOnScreenFrame(out, info.presentationTimeUs, key, codec)
                    }
                }
                c.releaseOutputBuffer(index, false)
            } catch (e: Throwable) {
                Log.w(TAG, "a frame was lost: ${e.message}")
            }
        }

        override fun onError(c: MediaCodec, e: MediaCodec.CodecException) {
            Log.e(TAG, "the encoder stopped: ${e.message}")
            handler?.post {
                stopScreen()
            }
        }

        override fun onOutputFormatChanged(c: MediaCodec, format: MediaFormat) = Unit
    }

    private fun stopEncoder() {
        try {
            encoder?.stop()
        } catch (_: Throwable) {
        }
        try {
            encoder?.release()
        } catch (_: Throwable) {
        }
        try {
            surface?.release()
        } catch (_: Throwable) {
        }
        encoder = null
        surface = null
    }

    private fun resize(width: Int, height: Int) {
        if (!showing || width <= 0 || height <= 0) return
        try {
            val (w, h) = fit(width, height)
            stopEncoder()
            val s = startEncoder(w, h)
            display?.surface = s
            display?.resize(w, h, realMetrics().densityDpi)
            SentinelCallPrimitives.nativeOnScreen(true, w, h)
        } catch (e: Throwable) {
            Log.e(TAG, "could not follow the new size: ${e.message}", e)
            stopScreen()
        }
    }

    fun wantWholePicture() {
        try {
            encoder?.setParameters(Bundle().apply { putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0) })
        } catch (e: Throwable) {
            Log.w(TAG, "no whole picture: ${e.message}")
        }
    }

    fun stopScreen() {
        val was = showing
        showing = false
        try {
            display?.release()
        } catch (_: Throwable) {
        }
        display = null
        stopEncoder()
        try {
            projection?.stop()
        } catch (_: Throwable) {
        }
        projection = null
        worker?.quitSafely()
        worker = null
        handler = null
        if (was) SentinelCallPrimitives.nativeOnScreen(false, 0, 0)
    }

    override fun onDestroy() {
        stopScreen()
        current = null
        super.onDestroy()
    }

    companion object {
        const val ACTION_CALL = "com.mobilesentinel.call.CALL"
        const val ACTION_SCREEN = "com.mobilesentinel.call.SCREEN"
        const val ACTION_STOP_SCREEN = "com.mobilesentinel.call.STOP_SCREEN"
        const val EXTRA_TITLE = "title"
        const val EXTRA_TEXT = "text"
        const val EXTRA_CAMERA = "camera"
        const val EXTRA_RESULT = "result"
        const val EXTRA_DATA = "data"

        private const val TAG = "MobileSentinel.Call"
        private const val CHANNEL = "sentinel_call"
        private const val NOTIFICATION_ID = 7302
        private const val MAX_SIDE = 1600
        private const val BITRATE = 2_500_000
        private const val FPS = 10
        private const val WHOLE_PICTURE_EVERY = 8

        @Volatile
        var current: SentinelCallService? = null

        @Volatile
        var stopLabel: String = "Stop"
    }
}
