package com.mobilesentinel

import android.Manifest
import android.content.pm.PackageManager
import android.media.MediaRecorder
import android.os.SystemClock
import android.util.Log
import org.json.JSONObject
import java.io.File
import java.util.UUID

/**
 * Microphone recording primitives (voice notes).
 *
 * Rust calls INTO these via JNI (`mobile_sentinel::recorder`). One recording
 * at a time, AAC in an `.m4a` container. Every method is defensive: failures
 * are logged under [TAG] and reported through the return value, never thrown.
 */
object SentinelRecorderPrimitives {
    private const val TAG = "MobileSentinel.Recorder"
    private const val PERMISSION_REQUEST_CODE = 9041

    private var recorder: MediaRecorder? = null
    private var file: File? = null
    private var startedAt = 0L

    @JvmStatic
    fun hasPermission(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            c.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        } catch (e: Throwable) {
            false
        }
    }

    /** Shows the system permission dialog if needed. Returns immediately. */
    @JvmStatic
    fun requestPermission() {
        if (hasPermission()) return
        val activity = SentinelActivityTracker.currentResumedActivity ?: run {
            Log.w(TAG, "requestPermission: no resumed Activity")
            return
        }
        activity.runOnUiThread {
            try {
                activity.requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), PERMISSION_REQUEST_CODE)
            } catch (e: Throwable) {
                Log.w(TAG, "requestPermission failed: ${e.message}")
            }
        }
    }

    /**
     * Start recording into a new file in [dir].
     * @return 0 = recording, 1 = permission missing, 2 = failed, 3 = busy.
     */
    @JvmStatic
    @Synchronized
    fun start(dir: String): Int {
        val ctx = SentinelPrimitives.getAppContext() ?: return 2
        if (!hasPermission()) return 1
        if (recorder != null) return 3
        val target = File(File(dir).apply { mkdirs() }, "${UUID.randomUUID()}.m4a")
        @Suppress("DEPRECATION")
        val r = MediaRecorder(ctx)
        return try {
            r.setAudioSource(MediaRecorder.AudioSource.MIC)
            r.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
            r.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
            r.setAudioChannels(1)
            r.setAudioSamplingRate(44100)
            r.setAudioEncodingBitRate(64000)
            r.setOutputFile(target.absolutePath)
            r.prepare()
            r.start()
            recorder = r
            file = target
            startedAt = SystemClock.elapsedRealtime()
            Log.i(TAG, "recording to ${target.absolutePath}")
            0
        } catch (e: Throwable) {
            Log.e(TAG, "start failed: ${e.message}")
            try { r.release() } catch (_: Throwable) {}
            target.delete()
            2
        }
    }

    /** Peak input level since the last call, 0.0–1.0. */
    @JvmStatic
    fun level(): Float {
        val r = recorder ?: return 0f
        return try {
            (r.maxAmplitude / 32767f).coerceIn(0f, 1f)
        } catch (e: Throwable) {
            0f
        }
    }

    @JvmStatic
    fun isRecording(): Boolean = recorder != null

    /**
     * Stop and keep the recording.
     * @return JSON {"path", "duration_ms"}, or "" if it failed (for example
     *   when stopped before any audio was captured).
     */
    @JvmStatic
    @Synchronized
    fun stop(): String {
        val r = recorder ?: return ""
        val f = file
        recorder = null
        file = null
        val duration = SystemClock.elapsedRealtime() - startedAt
        return try {
            r.stop()
            r.release()
            if (f == null || !f.exists() || f.length() == 0L) {
                ""
            } else {
                JSONObject().put("path", f.absolutePath).put("duration_ms", duration).toString()
            }
        } catch (e: Throwable) {
            // stop() throws when no valid audio was recorded yet.
            Log.w(TAG, "stop failed: ${e.message}")
            try { r.release() } catch (_: Throwable) {}
            f?.delete()
            ""
        }
    }

    /** Stop and delete the recording. */
    @JvmStatic
    @Synchronized
    fun cancel() {
        val r = recorder ?: return
        val f = file
        recorder = null
        file = null
        try { r.stop() } catch (_: Throwable) {}
        try { r.release() } catch (_: Throwable) {}
        f?.delete()
    }
}
