package com.mobilesentinel

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
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
     * Compose an email in the user's own email app: ACTION_SEND_MULTIPLE with the files as
     * content:// URIs. [paths] is one absolute path per line (may be empty).
     * @return true if the email app was opened, false if there is none or it failed.
     */
    @JvmStatic
    fun shareEmail(to: String, subject: String, body: String, paths: String): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            val uris = ArrayList<Uri>()
            for (path in paths.split("\n").filter { it.isNotEmpty() }) {
                uris.add(uriFor(context, File(path)))
            }
            val intent = Intent(Intent.ACTION_SEND_MULTIPLE).apply {
                type = "message/rfc822"
                if (to.isNotEmpty()) putExtra(Intent.EXTRA_EMAIL, arrayOf(to))
                putExtra(Intent.EXTRA_SUBJECT, subject)
                putExtra(Intent.EXTRA_TEXT, body)
                putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                if (uris.isNotEmpty()) {
                    val clip = android.content.ClipData.newRawUri("", uris[0])
                    for (i in 1 until uris.size) clip.addItem(android.content.ClipData.Item(uris[i]))
                    clipData = clip
                }
            }
            if (intent.resolveActivity(context.packageManager) == null) {
                Log.i(TAG, "shareEmail: no email app")
                return false
            }
            val chooser = Intent.createChooser(intent, subject).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(chooser)
            true
        } catch (e: Throwable) {
            Log.w(TAG, "shareEmail: ${e.message}", e)
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
            providerUri(context, file)
        } catch (e: IllegalArgumentException) {
            // A file outside the provider's roots, such as the device-protected files of an app
            // that starts before the first unlock: share a copy from the cache.
            val copy = File(File(context.cacheDir, "shared").apply { mkdirs() }, file.name)
            file.copyTo(copy, overwrite = true)
            providerUri(context, copy)
        }
    }

    private fun providerUri(context: Context, file: File): Uri {
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
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return saveToDownloadsFolder(context, src, name)
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

    /** Before Android 10 there is no MediaStore.Downloads: the folder itself, which needs the storage permission. */
    @Suppress("DEPRECATION")
    private fun saveToDownloadsFolder(context: Context, src: File, name: String): String = try {
        if (!legacyStorageGranted(context)) {
            Log.w(TAG, "saveToDownloads(${src.path}): no storage permission")
            ""
        } else {
            val dir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
            dir.mkdirs()
            src.copyTo(File(dir, name), overwrite = true)
            "Download/$name"
        }
    } catch (e: Throwable) {
        Log.w(TAG, "saveToDownloads(${src.path}): ${e.message}", e)
        ""
    }

    /**
     * Whether saving to Downloads needs a permission that is not granted: up to Android 9 the public folder needs
     * WRITE_EXTERNAL_STORAGE (a runtime permission; the capability declares it up to API 28 only), from Android 10 nothing is
     * needed. A host asks this after a save failed, to tell the person to allow it.
     */
    @JvmStatic
    fun downloadsPermissionMissing(): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return Build.VERSION.SDK_INT < Build.VERSION_CODES.Q && !storageGranted(context)
    }

    private fun storageGranted(context: Context) =
        context.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED

    /**
     * When the permission is missing the system dialog is asked for, through the resumed activity, and the call says no at
     * once: the answer comes whenever the person gives it, on the UI thread of the app, and nothing here may wait for it
     * (the dialog pauses the activity, and the caller can be the very thread that has to handle that).
     */
    private fun legacyStorageGranted(context: Context): Boolean {
        if (storageGranted(context)) return true
        val activity = SentinelActivityTracker.currentResumedActivity ?: return false
        activity.runOnUiThread {
            try {
                activity.requestPermissions(arrayOf(Manifest.permission.WRITE_EXTERNAL_STORAGE), STORAGE_REQUEST_CODE)
            } catch (e: Throwable) {
                Log.w(TAG, "storage permission dialog: ${e.message}")
            }
        }
        return false
    }

    private const val STORAGE_REQUEST_CODE = 0x5E12
}

/** The share capability's own FileProvider (a subclass, so it can coexist
 *  with the camera capability's provider in one manifest). */
class SentinelShareFileProvider : FileProvider()
