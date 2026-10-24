package com.mobilesentinel

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.MediaPlayer
import android.media.RingtoneManager
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import java.io.File

/**
 * Firing-audio primitives — the alarm playback surface (play/stop a looping
 * sound while an alarm fires).
 *
 * Rust calls INTO these via JNI (the firing sink, gated by the `firing-audio`
 * Cargo feature). Rust resolves WHICH uri to play (via the Sound Library) and
 * decides when to start/stop; this object just drives a single `MediaPlayer`,
 * optionally fading it in and keeping the alarm stream audible.
 * Every method is defensive: logs and returns a safe fallback on failure.
 */
object SentinelFiringAudioPrimitives {
    private const val TAG = "MobileSentinel.FiringAudio"
    private const val FADE_STEP_MS = 100L

    /** Primary firing-audio player; replaced on each [playSound] call. */
    @Volatile
    private var primaryPlayer: MediaPlayer? = null

    private val fadeHandler = Handler(Looper.getMainLooper())

    /** The alarm-stream volume to put back when the sound stops; -1 when we changed nothing. */
    @Volatile
    private var volumeToRestore = -1

    @JvmStatic
    fun playSound(
        uri: String,
        usage: String,
        contentType: String,
        looping: Boolean,
        fadeInMs: Long,
        minVolumePercent: Int,
    ): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            stopSound() // ensure single player
            val resolved = resolveAudioUri(uri) ?: return false
            Log.i(TAG, "playSound uri='$uri' resolved='$resolved' usage=$usage looping=$looping fadeIn=${fadeInMs}ms minVolume=$minVolumePercent%")
            ensureMinVolume(c, minVolumePercent)
            val attrs = AudioAttributes.Builder()
                .setUsage(parseAudioUsage(usage))
                .setContentType(parseAudioContentType(contentType))
                .build()
            val mp = MediaPlayer().apply {
                setAudioAttributes(attrs)
                setDataSource(c, resolved)
                isLooping = looping
                setOnErrorListener { _, what, extra ->
                    Log.w(TAG, "MediaPlayer error what=$what extra=$extra")
                    false
                }
                prepare()
                if (fadeInMs > 0) setVolume(0f, 0f)
                start()
            }
            primaryPlayer = mp
            if (fadeInMs > 0) startFade(mp, fadeInMs)
            true
        } catch (e: Throwable) {
            Log.e(TAG, "playSound: ${e.message}", e)
            false
        }
    }

    /** Ramp [mp] from silent to full volume over [fadeInMs]. */
    private fun startFade(mp: MediaPlayer, fadeInMs: Long) {
        val startedAt = SystemClock.uptimeMillis()
        val step = object : Runnable {
            override fun run() {
                if (primaryPlayer !== mp) return // stopped or replaced
                val level = ((SystemClock.uptimeMillis() - startedAt).toFloat() / fadeInMs).coerceIn(0f, 1f)
                try {
                    mp.setVolume(level, level)
                } catch (_: Throwable) {
                    return
                }
                if (level < 1f) fadeHandler.postDelayed(this, FADE_STEP_MS)
            }
        }
        fadeHandler.post(step)
    }

    /**
     * Raise the alarm stream to at least [percent] of its range, remembering
     * the old level for [stopSound]. Does nothing for 0, or when the stream
     * is already loud enough. Some devices refuse the change while Do Not
     * Disturb is on; that is logged and the alarm plays at the user's volume.
     */
    private fun ensureMinVolume(c: Context, percent: Int) {
        if (percent <= 0) return
        try {
            val am = c.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            val max = am.getStreamMaxVolume(AudioManager.STREAM_ALARM)
            val target = Math.ceil(max * percent.coerceAtMost(100) / 100.0).toInt().coerceIn(1, max)
            val current = am.getStreamVolume(AudioManager.STREAM_ALARM)
            if (current < target) {
                if (volumeToRestore < 0) volumeToRestore = current
                am.setStreamVolume(AudioManager.STREAM_ALARM, target, 0)
                Log.i(TAG, "alarm volume raised $current -> $target (of $max)")
            }
        } catch (e: Throwable) {
            Log.w(TAG, "ensureMinVolume: ${e.message}")
        }
    }

    private fun restoreVolume() {
        val saved = volumeToRestore
        if (saved < 0) return
        volumeToRestore = -1
        try {
            val c = SentinelPrimitives.getAppContext() ?: return
            val am = c.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            am.setStreamVolume(AudioManager.STREAM_ALARM, saved, 0)
            Log.i(TAG, "alarm volume restored to $saved")
        } catch (e: Throwable) {
            Log.w(TAG, "restoreVolume: ${e.message}")
        }
    }

    @JvmStatic
    fun stopSound(): Boolean {
        return try {
            fadeHandler.removeCallbacksAndMessages(null)
            primaryPlayer?.let { mp ->
                try {
                    if (mp.isPlaying) mp.stop()
                } catch (_: Throwable) {
                }
                try {
                    mp.release()
                } catch (_: Throwable) {
                }
            }
            primaryPlayer = null
            restoreVolume()
            true
        } catch (e: Throwable) {
            Log.e(TAG, "stopSound: ${e.message}", e)
            false
        }
    }

    private fun resolveAudioUri(uri: String): Uri? {
        return try {
            when {
                uri == "system://default-alarm" -> RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
                uri.startsWith("file://") || uri.startsWith("content://") -> Uri.parse(uri)
                uri.startsWith("/") -> Uri.fromFile(File(uri))
                else -> Uri.parse(uri)
            }
        } catch (e: Throwable) {
            Log.w(TAG, "resolveAudioUri($uri): ${e.message}")
            null
        }
    }

    private fun parseAudioUsage(usage: String): Int = when (usage.lowercase()) {
        "alarm" -> AudioAttributes.USAGE_ALARM
        "notification" -> AudioAttributes.USAGE_NOTIFICATION
        "media", "music" -> AudioAttributes.USAGE_MEDIA
        else -> AudioAttributes.USAGE_ALARM
    }

    private fun parseAudioContentType(t: String): Int = when (t.lowercase()) {
        "sonification" -> AudioAttributes.CONTENT_TYPE_SONIFICATION
        "music" -> AudioAttributes.CONTENT_TYPE_MUSIC
        "speech" -> AudioAttributes.CONTENT_TYPE_SPEECH
        else -> AudioAttributes.CONTENT_TYPE_SONIFICATION
    }
}
