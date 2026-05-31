package com.mobilesentinel

import android.content.Intent
import android.net.Uri
import android.util.Log
import androidx.core.content.FileProvider
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Camera capture helper: launches the system camera app to take a photo and
 * returns the absolute path of the saved JPEG.
 *
 * Flow (mirrors the file picker — proven, freeze-free):
 * 1. Rust calls [captureFromAppContext] on a background thread (via JNI).
 * 2. This helper creates an empty target file under `<files>/sentinel_camera/`
 *    and a `content://` FileProvider URI pointing at it.
 * 3. It launches [SentinelCameraActivity], which fires `ACTION_IMAGE_CAPTURE`.
 * 4. The camera app writes the full-res JPEG into the target file.
 * 5. The blocking call returns the target file's absolute path (or empty on
 *    cancel / failure / timeout).
 */
object SentinelCameraHelper {

    private const val TAG = "MobileSentinel.Camera"
    private const val CAPTURE_DIR = "sentinel_camera"

    private var latch: CountDownLatch? = null
    private val captured = AtomicBoolean(false)

    /**
     * Take a photo via the system camera and block until done.
     * MUST be called from a background thread (not the UI thread).
     * @return absolute path of the saved JPEG, or empty string on
     *   cancel / failure / timeout.
     */
    @JvmStatic
    fun captureFromAppContext(timeoutSeconds: Long = 120): String {
        val ctx = SentinelPrimitives.getAppContext()
            ?: run {
                Log.e(TAG, "captureFromAppContext: no appContext")
                return ""
            }

        // Pre-create the destination file.
        val dir = File(ctx.filesDir, CAPTURE_DIR).apply { mkdirs() }
        val dest = File(dir, "${UUID.randomUUID()}.jpg")
        val authority = "${ctx.packageName}.sentinelfileprovider"
        val uri = try {
            FileProvider.getUriForFile(ctx, authority, dest)
        } catch (e: Exception) {
            Log.e(TAG, "FileProvider.getUriForFile failed: ${e.message}")
            return ""
        }

        captured.set(false)
        latch = CountDownLatch(1)

        // Launch the proxy. Prefer the current activity (same task) over the
        // app context (which needs NEW_TASK and spawns a separate task).
        val activity = SentinelActivityTracker.currentResumedActivity
        val intent = Intent(
            activity ?: ctx,
            SentinelCameraActivity::class.java
        ).apply {
            putExtra(SentinelCameraActivity.EXTRA_OUTPUT_URI, uri)
            if (activity == null) addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        try {
            if (activity != null) {
                activity.runOnUiThread { activity.startActivity(intent) }
            } else {
                ctx.startActivity(intent)
            }
            Log.i(TAG, "Camera proxy launched")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to launch camera proxy: ${e.message}")
            return ""
        }

        val completed = latch?.await(timeoutSeconds, TimeUnit.SECONDS) ?: false
        if (!completed) {
            Log.w(TAG, "Camera capture timed out after ${timeoutSeconds}s")
            dest.delete()
            return ""
        }

        return if (captured.get() && dest.exists() && dest.length() > 0) {
            Log.i(TAG, "Photo captured: ${dest.absolutePath} (${dest.length()} bytes)")
            dest.absolutePath
        } else {
            // Cancelled or empty — clean up the stub file.
            dest.delete()
            Log.i(TAG, "Capture cancelled / empty")
            ""
        }
    }

    /** Called by [SentinelCameraActivity] when capture completes or cancels. */
    @JvmStatic
    fun deliverResult(success: Boolean) {
        captured.set(success)
        latch?.countDown()
    }
}
