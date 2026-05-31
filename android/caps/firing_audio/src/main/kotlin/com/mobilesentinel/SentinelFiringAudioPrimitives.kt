package com.mobilesentinel

import android.media.AudioAttributes
import android.media.MediaPlayer
import android.media.RingtoneManager
import android.net.Uri
import android.util.Log
import java.io.File

/**
 * Firing-audio primitives — the alarm playback surface (play/stop a looping
 * sound while an alarm fires).
 *
 * Rust calls INTO these via JNI (the firing sink, gated by the `firing-audio`
 * Cargo feature). Rust resolves WHICH uri to play (via the Sound Library) and
 * decides when to start/stop; this object just drives a single `MediaPlayer`.
 * Every method is defensive: logs and returns a safe fallback on failure.
 */
object SentinelFiringAudioPrimitives {
    private const val TAG = "MobileSentinel.FiringAudio"

    /** Primary firing-audio player; replaced on each [playSound] call. */
    @Volatile
    private var primaryPlayer: MediaPlayer? = null

    @JvmStatic
    fun playSound(uri: String, usage: String, contentType: String, looping: Boolean): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            stopSound() // ensure single player
            val resolved = resolveAudioUri(uri) ?: return false
            Log.i(TAG, "playSound uri='$uri' resolved='$resolved' usage=$usage looping=$looping")
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
                start()
            }
            primaryPlayer = mp
            true
        } catch (e: Throwable) {
            Log.e(TAG, "playSound: ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun stopSound(): Boolean {
        return try {
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
