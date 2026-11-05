package com.mobilesentinel

import android.content.Context
import android.content.Intent
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.util.Log

/**
 * What a live call needs from Android: a foreground service, so the microphone and camera go on
 * while the app is not in front; showing the screen; and which speaker the sound comes out of.
 *
 * The Rust side lives in `src/features/call_service.rs`.
 */
object SentinelCallPrimitives {
    private const val TAG = "MobileSentinel.Call"

    /** Starts or updates the service. Android only allows this while the app is in front. */
    @JvmStatic
    fun startCall(title: String, text: String, camera: Boolean): Boolean = try {
        val ctx = SentinelPrimitives.getAppContext() ?: throw IllegalStateException("no context")
        val intent = Intent(ctx, SentinelCallService::class.java)
            .setAction(SentinelCallService.ACTION_CALL)
            .putExtra(SentinelCallService.EXTRA_TITLE, title)
            .putExtra(SentinelCallService.EXTRA_TEXT, text)
            .putExtra(SentinelCallService.EXTRA_CAMERA, camera)
        ctx.startForegroundService(intent)
        true
    } catch (e: Throwable) {
        Log.e(TAG, "startCall failed: ${e.message}", e)
        false
    }

    @JvmStatic
    fun stopCall() {
        try {
            val ctx = SentinelPrimitives.getAppContext() ?: return
            ctx.stopService(Intent(ctx, SentinelCallService::class.java))
        } catch (e: Throwable) {
            Log.w(TAG, "stopCall: ${e.message}", e)
        }
    }

    /**
     * Asks Android whether the screen may be shown. The answer comes through [nativeOnScreen] and
     * the frames through [nativeOnScreenFrame]. Android asks every time.
     */
    @JvmStatic
    fun askForScreen(stopLabel: String): Boolean = try {
        val ctx = SentinelPrimitives.getAppContext() ?: throw IllegalStateException("no context")
        SentinelCallService.stopLabel = stopLabel
        ctx.startActivity(
            Intent(ctx, SentinelScreenConsentActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        true
    } catch (e: Throwable) {
        Log.e(TAG, "askForScreen failed: ${e.message}", e)
        false
    }

    @JvmStatic
    fun stopScreen() {
        SentinelCallService.current?.stopScreen()
    }

    /** Asks the encoder for a whole picture at the next frame. */
    @JvmStatic
    fun wantWholePicture() {
        SentinelCallService.current?.wantWholePicture()
    }

    /** A call's sound: from the speaker, or from whatever the system picks (earpiece, headset). */
    @JvmStatic
    fun setSpeaker(on: Boolean): Boolean = try {
        val ctx = SentinelPrimitives.getAppContext() ?: throw IllegalStateException("no context")
        val audio = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        audio.mode = AudioManager.MODE_IN_COMMUNICATION
        if (on) {
            val speaker = audio.availableCommunicationDevices
                .firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER }
            speaker != null && audio.setCommunicationDevice(speaker)
        } else {
            audio.clearCommunicationDevice()
            true
        }
    } catch (e: Throwable) {
        Log.w(TAG, "setSpeaker: ${e.message}", e)
        false
    }

    /** The call is over: sound goes back to normal. */
    @JvmStatic
    fun endAudio() {
        try {
            val ctx = SentinelPrimitives.getAppContext() ?: return
            val audio = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            audio.clearCommunicationDevice()
            audio.mode = AudioManager.MODE_NORMAL
        } catch (e: Throwable) {
            Log.w(TAG, "endAudio: ${e.message}", e)
        }
    }

    /** Showing the screen started (with its size) or stopped. */
    @JvmStatic
    external fun nativeOnScreen(on: Boolean, width: Int, height: Int)

    /** One encoded frame. [codec]: 2 H.264, 0 VP8. */
    @JvmStatic
    external fun nativeOnScreenFrame(data: ByteArray, timeUs: Long, key: Boolean, codec: Int)
}
