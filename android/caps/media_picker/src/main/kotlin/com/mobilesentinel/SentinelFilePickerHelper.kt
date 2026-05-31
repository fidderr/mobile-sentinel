package com.mobilesentinel

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.util.Log
import android.webkit.MimeTypeMap
import java.io.File
import java.io.FileOutputStream
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/**
 * Transparent activity that launches Android's document picker.
 * Accepts a MIME type via intent extra. Finishes itself after the user picks or cancels.
 */
class SentinelFilePickerActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val mimeType = intent.getStringExtra(EXTRA_MIME_TYPE) ?: "application/octet-stream"
        val pickIntent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = mimeType
        }
        try {
            startActivityForResult(pickIntent, REQUEST_CODE)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to launch file picker: ${e.message}")
            SentinelFilePickerHelper.deliverResult(null)
            finish()
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQUEST_CODE) {
            val uri = if (resultCode == RESULT_OK) data?.data else null
            SentinelFilePickerHelper.deliverResult(uri)
            finish()
        }
    }

    companion object {
        private const val TAG = "MobileSentinel.FilePicker"
        private const val REQUEST_CODE = 9020
        const val EXTRA_MIME_TYPE = "com.mobilesentinel.EXTRA_MIME_TYPE"
    }
}

/**
 * Generic file picker helper for Android.
 * Launches the system document picker, copies the selected file to the app's
 * internal storage, and returns the full path. The consumer app decides what
 * to do with the file afterwards.
 * Flow:
 * 1. Consumer calls `pickFileBlocking(activity, mimeType, timeout)` from a background thread
 * 2. This helper launches SentinelFilePickerActivity (transparent)
 * 3. That activity launches Android's document picker filtered by MIME type
 * 4. User selects a file
 * 5. The file is copied to the app's internal storage with a unique name
 * 6. The blocking call returns the full internal file path
 * Uses a CountDownLatch so the JNI call can block until the user completes picking.
 */
object SentinelFilePickerHelper {

    private const val TAG = "MobileSentinel.FilePicker"
    private const val IMPORT_DIR = "sentinel_imports"

    private var pickLatch: CountDownLatch? = null
    private val pickResult = AtomicReference<Uri?>(null)

    /**
 * Launch the file picker and block until a result is available.
 * MUST be called from a background thread (not the UI thread).
 * @param activity The current activity (used for context and launching)
 * @param mimeType MIME type filter (e.g. "audio/star", "image/star")
 * @param timeoutSeconds Maximum time to wait for pick result
 * @return The full internal file path of the copied file, or empty string if cancelled/timeout
 */
    @JvmStatic
    fun pickFileBlocking(activity: Activity, mimeType: String, timeoutSeconds: Long = 120): String {
        pickResult.set(null)
        pickLatch = CountDownLatch(1)

 // Launch the transparent picker activity with the MIME type
        val intent = Intent(activity, SentinelFilePickerActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK
            putExtra(SentinelFilePickerActivity.EXTRA_MIME_TYPE, mimeType)
        }
        activity.startActivity(intent)
        Log.i(TAG, "File picker launched (mimeType=$mimeType)")

 // Block until result or timeout
        val completed = pickLatch?.await(timeoutSeconds, TimeUnit.SECONDS) ?: false
        if (!completed) {
            Log.w(TAG, "File picker timed out after ${timeoutSeconds}s")
            return ""
        }

        val uri = pickResult.get() ?: return ""

 // Copy the selected file to internal storage
        return try {
            copyToInternal(activity, uri)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to copy picked file: ${e.message}")
            ""
        }
    }

    /**
 * Called by SentinelFilePickerActivity when the user picks or cancels.
 */
    @JvmStatic
    fun deliverResult(uri: Uri?) {
        pickResult.set(uri)
        if (uri != null) {
            Log.i(TAG, "File picked: $uri")
        } else {
            Log.i(TAG, "File pick cancelled")
        }
        pickLatch?.countDown()
    }

    /**
 * Copy a content:// URI to the app's internal import directory.
 * Returns the full path to the copied file.
 */
    private fun copyToInternal(activity: Activity, uri: Uri): String {
        val importDir = File(activity.filesDir, IMPORT_DIR)
        importDir.mkdirs()

 // Determine extension from MIME type using Android's MimeTypeMap
        val mimeType = activity.contentResolver.getType(uri)
        val ext = if (mimeType != null) {
            MimeTypeMap.getSingleton().getExtensionFromMimeType(mimeType)
                ?: extractExtensionFromUri(uri)
                ?: "bin"
        } else {
            extractExtensionFromUri(uri) ?: "bin"
        }

        val uniqueName = "${UUID.randomUUID()}.$ext"
        val destFile = File(importDir, uniqueName)

        activity.contentResolver.openInputStream(uri)?.use { input ->
            FileOutputStream(destFile).use { output ->
                input.copyTo(output)
            }
        } ?: throw Exception("Could not open input stream for $uri")

        Log.i(TAG, "Copied file to: ${destFile.absolutePath} (${destFile.length()} bytes, mime=$mimeType)")
        return destFile.absolutePath
    }

    /**
 * Try to extract a file extension from the URI's last path segment.
 */
    private fun extractExtensionFromUri(uri: Uri): String? {
        val path = uri.lastPathSegment ?: return null
        val dotIdx = path.lastIndexOf('.')
        return if (dotIdx >= 0 && dotIdx < path.length - 1) {
            path.substring(dotIdx + 1).lowercase()
        } else {
            null
        }
    }
}
