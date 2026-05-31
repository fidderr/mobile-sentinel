package com.mobilesentinel

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.util.Log

/**
 * Clipboard platform primitives.
 *
 * Rust calls INTO these functions via JNI. Each method is a thin wrapper
 * around Android's [ClipboardManager] — no orchestration logic, no state.
 * Reached from Rust via `primitives_ext::clipboard_*`.
 *
 * Every method is total: it wraps its body in try/catch, logs failures,
 * and returns a safe fallback. None of them ever throw.
 */
object SentinelClipboardPrimitives {
    private const val TAG = "MobileSentinel.Clipboard"

    /** Resolve the system [ClipboardManager], or null when unavailable. */
    private fun clipboard(): ClipboardManager? {
        val ctx: Context = SentinelPrimitives.getAppContext() ?: return null
        return ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
    }

    /**
     * Place [text] on the clipboard as plain text under the "sentinel" label.
     * Returns true on success, false if there is no context or any error.
     */
    @JvmStatic
    fun setText(text: String): Boolean {
        return try {
            val clipboard = clipboard() ?: return false
            val clip = ClipData.newPlainText("sentinel", text)
            clipboard.setPrimaryClip(clip)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "setText: ${e.message}", e)
            false
        }
    }

    /**
     * Read the clipboard's primary text. Returns null when there is no
     * context, no primary clip, or the first item carries no text.
     */
    @JvmStatic
    fun getText(): String? {
        return try {
            val clipboard = clipboard() ?: return null
            if (!clipboard.hasPrimaryClip()) return null
            clipboard.primaryClip?.getItemAt(0)?.text?.toString()
        } catch (e: Throwable) {
            Log.w(TAG, "getText: ${e.message}", e)
            null
        }
    }

    /**
     * Whether the clipboard currently holds text. Returns false when there
     * is no context or any error occurs.
     */
    @JvmStatic
    fun hasText(): Boolean {
        return try {
            clipboard()?.hasText() ?: false
        } catch (e: Throwable) {
            Log.w(TAG, "hasText: ${e.message}", e)
            false
        }
    }
}
