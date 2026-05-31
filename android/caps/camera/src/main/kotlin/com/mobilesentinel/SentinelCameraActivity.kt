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
            val capture = Intent(MediaStore.ACTION_IMAGE_CAPTURE).apply {
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
            SentinelCameraHelper.deliverResult(resultCode == RESULT_OK)
            finish()
        }
    }

    companion object {
        private const val TAG = "MobileSentinel.CameraAct"
        private const val REQUEST_CODE = 9030
        const val EXTRA_OUTPUT_URI = "com.mobilesentinel.EXTRA_OUTPUT_URI"
    }
}
