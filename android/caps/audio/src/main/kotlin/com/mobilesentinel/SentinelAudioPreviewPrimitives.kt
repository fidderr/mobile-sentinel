package com.mobilesentinel

import android.media.AudioAttributes
import android.media.MediaPlayer
import android.net.Uri
import android.util.Log
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * Handle-based audio playback for UI sound PREVIEW.
 *
 * This is intentionally distinct from the firing-alarm playback surface
 * ([SentinelFiringAudioPrimitives.playSound]), which owns a single primary
 * player and is driven by the AlarmKit state machine. Preview playback is a
 * separate concern: a consumer (e.g. a sound-picker screen) plays a candidate sound,
 * receives an opaque `Long` handle, and later stops that specific handle.
 *
 * Multiple previews can be active at once. Each call to [play] allocates a
 * fresh, monotonically increasing handle id (starting at 1) and stores the
 * backing [MediaPlayer] in [players]. [stop] removes and releases the player
 * for a given handle. Non-looping players self-release on completion via an
 * `OnCompletionListener`, removing themselves from the map.
 *
 * Threading: every method is `@JvmStatic`. State lives in the thread-safe
 * [players] map, the [nextHandle] counter, and the volatile [volume] field.
 * Every body is wrapped in try/catch — these primitives never throw; they
 * return a safe fallback (`0` / `false`) and log a warning instead.
 */
object SentinelAudioPreviewPrimitives {
    private const val TAG = "MobileSentinel.AudioPreview"

    /** Active preview players keyed by handle id. */
    private val players = ConcurrentHashMap<Long, MediaPlayer>()

    /** Monotonic handle allocator. First handle is 1; 0 is reserved for "failure". */
    private val nextHandle = AtomicLong(1)

    /** Volume applied to every active player. Range 0.0..1.0. */
    @Volatile
    private var volume = 1.0f

    /**
     * Start previewing [uri].
     *
     * @param uri     `file://` / `content://` URI, an absolute file path
     *                (starting with `/`), or any other parseable URI string.
     * @param looping whether the preview should loop until [stop] is called.
     * @return a handle id (> 0) on success, or 0 on any failure.
     */
    @JvmStatic
    fun play(uri: String, looping: Boolean): Long {
        SentinelPrimitives.getAppContext() ?: return 0L
        var player: MediaPlayer? = null
        return try {
            val attrs = AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build()
            val mp = MediaPlayer()
            player = mp
            mp.setAudioAttributes(attrs)
            // Resolve the data source: absolute paths use the String overload,
            // everything else is parsed as a Uri.
            when {
                uri.startsWith("file://") || uri.startsWith("content://") ->
                    mp.setDataSource(Uri.parse(uri).toString())
                uri.startsWith("/") -> mp.setDataSource(uri)
                else -> mp.setDataSource(Uri.parse(uri).toString())
            }
            mp.isLooping = looping
            mp.prepare()
            mp.start()
            val currentVolume = volume
            mp.setVolume(currentVolume, currentVolume)

            val id = nextHandle.getAndIncrement()
            players[id] = mp
            mp.setOnCompletionListener {
                if (!looping) {
                    try {
                        players.remove(id)?.release()
                    } catch (e: Throwable) {
                        Log.w(TAG, "play: onCompletion release failed for handle=$id: ${e.message}")
                    }
                }
            }
            Log.i(TAG, "play uri='$uri' looping=$looping -> handle=$id")
            id
        } catch (e: Throwable) {
            Log.w(TAG, "play('$uri', $looping): ${e.message}", e)
            try {
                player?.release()
            } catch (_: Throwable) {
            }
            0L
        }
    }

    /**
     * Stop and release the preview player for [handle].
     *
     * @return true if a player was found and stopped, false otherwise.
     */
    @JvmStatic
    fun stop(handle: Long): Boolean {
        return try {
            val mp = players.remove(handle) ?: return false
            try {
                mp.stop()
            } catch (e: Throwable) {
                Log.w(TAG, "stop: MediaPlayer.stop failed for handle=$handle: ${e.message}")
            }
            try {
                mp.release()
            } catch (e: Throwable) {
                Log.w(TAG, "stop: MediaPlayer.release failed for handle=$handle: ${e.message}")
            }
            Log.i(TAG, "stop handle=$handle")
            true
        } catch (e: Throwable) {
            Log.w(TAG, "stop($handle): ${e.message}", e)
            false
        }
    }

    /**
     * Set the preview volume for all active players.
     *
     * @param volume desired volume, clamped to 0.0..1.0.
     * @return true (always, unless an unexpected error occurs).
     */
    @JvmStatic
    fun setVolume(volume: Float): Boolean {
        return try {
            val clamped = volume.coerceIn(0.0f, 1.0f)
            this.volume = clamped
            for (mp in players.values) {
                try {
                    mp.setVolume(clamped, clamped)
                } catch (e: Throwable) {
                    Log.w(TAG, "setVolume: applying to player failed: ${e.message}")
                }
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setVolume($volume): ${e.message}", e)
            false
        }
    }
}
