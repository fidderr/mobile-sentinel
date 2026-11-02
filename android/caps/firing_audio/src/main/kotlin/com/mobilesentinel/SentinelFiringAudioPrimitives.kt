package com.mobilesentinel

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaPlayer
import android.media.RingtoneManager
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import java.io.File

/**
 * Firing-audio primitives — plays an alarm's looping sound, driven by the ring
 * service in `:sentinel`.
 *
 * Takes a list of sounds, best first, and falls through to the next when one
 * fails: on an error, and also when playback does not actually advance, because
 * a broken file can look like it plays without making a sound. The service says
 * what to do when nothing could be played. All calls on the main thread.
 */
object SentinelFiringAudioPrimitives {
    private const val TAG = "MobileSentinel.FiringAudio"
    private const val FADE_STEP_MS = 100L
    /** Seconds without progress before a sound counts as not playing. */
    private const val STUCK_AFTER_CHECKS = 3

    private var player: MediaPlayer? = null
    private val handler = Handler(Looper.getMainLooper())

    private var chain: List<String> = emptyList()
    private var next = 0
    private var usage = "alarm"
    private var contentType = "sonification"
    private var looping = true
    private var fadeInMs = 0L
    private var onExhausted: (() -> Unit)? = null

    private var lastPosition = -1
    private var stuckChecks = 0

    private var focusRequest: AudioFocusRequest? = null

    fun play(
        sounds: List<String>,
        usage: String,
        contentType: String,
        looping: Boolean,
        fadeInMs: Long,
        onExhausted: () -> Unit,
    ) {
        stop()
        chain = sounds
        next = 0
        this.usage = usage
        this.contentType = contentType
        this.looping = looping
        this.fadeInMs = fadeInMs
        this.onExhausted = onExhausted
        requestFocus()
        playNext()
    }

    private fun playNext() {
        releasePlayer()
        val c = SentinelPrimitives.getAppContext()
        while (c != null && next < chain.size) {
            val uri = chain[next++]
            try {
                player = open(c, uri)
                Log.i(TAG, "playing '$uri' looping=$looping fadeIn=${fadeInMs}ms")
                return
            } catch (e: Throwable) {
                Log.w(TAG, "'$uri' could not be played: ${e.message}")
            }
        }
        Log.e(TAG, "none of the ${chain.size} sound(s) could be played")
        onExhausted?.invoke()
        onExhausted = null
    }

    private fun open(c: Context, uri: String): MediaPlayer {
        val mp = MediaPlayer()
        try {
            mp.setAudioAttributes(attributes())
            mp.setDataSource(c, resolveAudioUri(uri))
            mp.isLooping = looping
            mp.setOnErrorListener { failed, what, extra ->
                Log.w(TAG, "MediaPlayer error what=$what extra=$extra")
                if (failed === player) playNext()
                true
            }
            mp.prepare()
            if (fadeInMs > 0) mp.setVolume(0f, 0f)
            mp.start()
        } catch (e: Throwable) {
            mp.release()
            throw e
        }
        lastPosition = -1
        stuckChecks = 0
        if (fadeInMs > 0) startFade(mp, fadeInMs)
        return mp
    }

    /** Called once a second: move on from a sound that is not actually playing. */
    fun checkPlayback() {
        val mp = player ?: return
        val position = try {
            if (mp.isPlaying) mp.currentPosition else -1
        } catch (_: Throwable) {
            -1
        }
        stuckChecks = if (position == -1 || position == lastPosition) stuckChecks + 1 else 0
        lastPosition = position
        if (stuckChecks < STUCK_AFTER_CHECKS) return
        stuckChecks = 0
        if (next < chain.size) {
            Log.w(TAG, "sound is not advancing, trying the next one")
            playNext()
        } else {
            // Nothing left to try. It may still be audible, so it plays on, but
            // the alarm must also be felt.
            Log.w(TAG, "last sound is not advancing")
            onExhausted?.invoke()
            onExhausted = null
        }
    }

    /** Ramp [mp] from silent to full volume over [fadeInMs]. */
    private fun startFade(mp: MediaPlayer, fadeInMs: Long) {
        val startedAt = SystemClock.uptimeMillis()
        val step = object : Runnable {
            override fun run() {
                if (player !== mp) return // stopped or replaced
                val level = ((SystemClock.uptimeMillis() - startedAt).toFloat() / fadeInMs).coerceIn(0f, 1f)
                try {
                    mp.setVolume(level, level)
                } catch (_: Throwable) {
                    return
                }
                if (level < 1f) handler.postDelayed(this, FADE_STEP_MS)
            }
        }
        handler.post(step)
    }

    /** Stop playing. Idempotent. */
    fun stop() {
        handler.removeCallbacksAndMessages(null)
        releasePlayer()
        chain = emptyList()
        onExhausted = null
        abandonFocus()
    }

    private fun releasePlayer() {
        val mp = player ?: return
        player = null
        try {
            if (mp.isPlaying) mp.stop()
        } catch (_: Throwable) {
        }
        try {
            mp.release()
        } catch (_: Throwable) {
        }
    }

    // Other audio should duck or stop for the alarm, but the alarm does not act
    // on losing focus: it is not allowed to go quiet because something else plays.
    private fun requestFocus() {
        val c = SentinelPrimitives.getAppContext() ?: return
        try {
            val am = c.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
                    .setAudioAttributes(attributes())
                    .setOnAudioFocusChangeListener { }
                    .build()
                focusRequest = request
                am.requestAudioFocus(request)
            } else {
                @Suppress("DEPRECATION")
                am.requestAudioFocus(null, AudioManager.STREAM_ALARM, AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
            }
        } catch (e: Throwable) {
            Log.w(TAG, "requestFocus: ${e.message}")
        }
    }

    private fun abandonFocus() {
        val c = SentinelPrimitives.getAppContext() ?: return
        try {
            val am = c.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                focusRequest?.let { am.abandonAudioFocusRequest(it) }
                focusRequest = null
            } else {
                @Suppress("DEPRECATION")
                am.abandonAudioFocus(null)
            }
        } catch (e: Throwable) {
            Log.w(TAG, "abandonFocus: ${e.message}")
        }
    }

    private fun attributes(): AudioAttributes = AudioAttributes.Builder()
        .setUsage(parseAudioUsage(usage))
        .setContentType(parseAudioContentType(contentType))
        .build()

    private fun resolveAudioUri(uri: String): Uri = when {
        uri == "system://default-alarm" -> RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
        uri.startsWith("/") -> Uri.fromFile(File(uri))
        else -> Uri.parse(uri)
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
