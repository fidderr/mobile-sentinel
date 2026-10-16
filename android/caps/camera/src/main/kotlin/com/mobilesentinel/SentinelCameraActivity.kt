package com.mobilesentinel

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import android.util.Log

/**
 * Transparent proxy activity that delegates photo capture to the device's
 * installed camera app via `ACTION_IMAGE_CAPTURE`.
 *
 * It is **translucent** (see the manifest theme) on purpose, for the same
 * reason the scanner is: a `singleInstance` host lives in a separate task, and
 * an opaque proxy would drive it to `onStop`, tearing down a WebView host's
 * render surface (→ frozen UI on return). A translucent proxy keeps the host
 * in `onPause`. The proxy itself shows nothing; the system camera app provides
 * the entire capture UI.
 *
 * The captured full-resolution JPEG is written by the camera app directly into
 * the pre-created file (handed over as a `content://` FileProvider URI), so no
 * bitmap marshalling is needed. Result is signalled back through
 * [SentinelCameraHelper.deliverResult].
 */
class SentinelCameraActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val outputUri: Uri? = intent.getParcelableExtra(EXTRA_OUTPUT_URI)
        if (outputUri == null) {
            Log.e(TAG, "no output uri")
            SentinelCameraHelper.deliverResult(false)
            finish()
            return
        }
        // Don't relaunch a kiosk over the system camera app.
        SentinelActivityTracker.suppressRelaunchFor(120_000)
        try {
            val action = if (intent.getBooleanExtra(EXTRA_VIDEO, false)) {
                MediaStore.ACTION_VIDEO_CAPTURE
            } else {
                MediaStore.ACTION_IMAGE_CAPTURE
            }
            val capture = Intent(action).apply {
                putExtra(MediaStore.EXTRA_OUTPUT, outputUri)
                addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            }
            startActivityForResult(capture, REQUEST_CODE)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to launch camera: ${e.message}")
            SentinelCameraHelper.deliverResult(false)
            finish()
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQUEST_CODE) {
            // Some camera apps ignore EXTRA_OUTPUT for video and hand back
            // their own URI instead: copy it into our target file.
            val returned = data?.data
            val target: Uri? = intent.getParcelableExtra(EXTRA_OUTPUT_URI)
            if (resultCode == RESULT_OK && returned != null && target != null && returned != target) {
                try {
                    contentResolver.openInputStream(returned)?.use { input ->
                        contentResolver.openOutputStream(target, "w")?.use { output ->
                            input.copyTo(output)
                        }
                    }
                } catch (e: Exception) {
                    Log.w(TAG, "copy of returned capture failed: ${e.message}")
                }
            }
            SentinelCameraHelper.deliverResult(resultCode == RESULT_OK)
            finish()
        }
    }

    companion object {
        private const val TAG = "MobileSentinel.CameraAct"
        private const val REQUEST_CODE = 9030
        const val EXTRA_OUTPUT_URI = "com.mobilesentinel.EXTRA_OUTPUT_URI"
        /** true = record a video (ACTION_VIDEO_CAPTURE) instead of a photo. */
        const val EXTRA_VIDEO = "com.mobilesentinel.EXTRA_VIDEO"
    }
}
