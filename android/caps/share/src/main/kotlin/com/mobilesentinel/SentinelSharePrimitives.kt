package com.mobilesentinel

import android.content.Intent
import android.util.Log
import androidx.core.content.FileProvider
import java.io.File

/**
 * Universal SDK share primitives.
 *
 * Rust calls INTO these functions via JNI to surface the Android share
 * sheet (`Intent.ACTION_SEND` + `Intent.createChooser`). Each method is a
 * thin wrapper around an Android API — no orchestration, no state.
 *
 * The application [android.content.Context] is obtained from
 * [SentinelPrimitives.getAppContext]; because these intents launch from a
 * non-Activity context, every chooser carries
 * [Intent.FLAG_ACTIVITY_NEW_TASK].
 *
 * Every method is defensive: any failure is logged under [TAG] and reported
 * as `false`. These methods never throw.
 */
object SentinelSharePrimitives {
    private const val TAG = "MobileSentinel.Share"

    /**
     * Share plain text via the system chooser.
     *
     * @param text  the text body placed in [Intent.EXTRA_TEXT].
     * @param title optional chooser title; defaults to `"Share"`.
     * @return true if the chooser was launched, false otherwise.
     */
    @JvmStatic
    fun shareText(text: String, title: String?): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val intent = Intent(Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(Intent.EXTRA_TEXT, text)
            }
            val chooser = Intent.createChooser(intent, title ?: "Share").apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(chooser)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "shareText: ${e.message}", e)
            false
        }
    }

    /**
     * Share a URL. On Android URLs are shared as plain text, so this simply
     * delegates to [shareText].
     */
    @JvmStatic
    fun shareUrl(url: String, title: String?): Boolean {
        return shareText(url, title)
    }

    /**
     * Share a file via a `content://` URI produced by [FileProvider].
     *
     * The FileProvider authority is `"<packageName>.fileprovider"`. If the
     * host app has not configured a matching FileProvider, the lookup throws
     * and this method logs and returns false rather than crashing.
     *
     * @param path     absolute path of the file to share.
     * @param mimeType MIME type advertised on the share intent.
     * @return true if the chooser was launched, false otherwise.
     */
    @JvmStatic
    fun shareFile(path: String, mimeType: String): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val authority = "${context.packageName}.fileprovider"
            val uri = FileProvider.getUriForFile(context, authority, File(path))
            val intent = Intent(Intent.ACTION_SEND).apply {
                type = mimeType
                putExtra(Intent.EXTRA_STREAM, uri)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            val chooser = Intent.createChooser(intent, "Share").apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(chooser)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "shareFile($path): ${e.message}", e)
            false
        }
    }
}
