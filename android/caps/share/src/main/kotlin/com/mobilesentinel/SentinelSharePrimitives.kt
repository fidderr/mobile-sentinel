package com.mobilesentinel

import android.content.ActivityNotFoundException
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
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
            val uri = uriFor(context, File(path))
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

    /**
     * A content:// URI for [file]: through the share capability's own
     * provider ("<package>.sentinelshareprovider", declared when the feature
     * is enabled), falling back to a host-declared "<package>.fileprovider".
     */
    private fun uriFor(context: Context, file: File): Uri {
        return try {
            FileProvider.getUriForFile(context, "${context.packageName}.sentinelshareprovider", file)
        } catch (e: IllegalArgumentException) {
            FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
        }
    }

    /**
     * Open a file in the app the user picks for its type (ACTION_VIEW), e.g.
     * a PDF viewer.
     * @return true if an app was launched, false if none can open it.
     */
    @JvmStatic
    fun openFile(path: String, mimeType: String): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val uri = uriFor(context, File(path))
            val intent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mimeType)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
            true
        } catch (e: ActivityNotFoundException) {
            Log.i(TAG, "openFile: no app for $mimeType")
            false
        } catch (e: Throwable) {
            Log.w(TAG, "openFile($path): ${e.message}", e)
            false
        }
    }

    /** Open a web address in the browser. @return true if an app was launched. */
    @JvmStatic
    fun openUrl(url: String): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
            true
        } catch (e: ActivityNotFoundException) {
            false
        } catch (e: Throwable) {
            Log.w(TAG, "openUrl: ${e.message}", e)
            false
        }
    }

    /**
     * Copy a file into the public Downloads folder under [name].
     * @return a human-readable location ("Download/name"), or "" on failure.
     */
    @JvmStatic
    fun saveToDownloads(path: String, name: String, mimeType: String): String {
        val context = SentinelPrimitives.getAppContext() ?: return ""
        val src = File(path)
        return try {
            val resolver = context.contentResolver
            val values = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, name)
                put(MediaStore.Downloads.MIME_TYPE, mimeType)
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val item = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: return ""
            resolver.openOutputStream(item)?.use { out ->
                src.inputStream().use { it.copyTo(out) }
            } ?: return ""
            values.clear()
            values.put(MediaStore.Downloads.IS_PENDING, 0)
            resolver.update(item, values, null, null)
            "Download/$name"
        } catch (e: Throwable) {
            Log.w(TAG, "saveToDownloads($path): ${e.message}", e)
            ""
        }
    }
}

/** The share capability's own FileProvider (a subclass, so it can coexist
 *  with the camera capability's provider in one manifest). */
class SentinelShareFileProvider : FileProvider()
