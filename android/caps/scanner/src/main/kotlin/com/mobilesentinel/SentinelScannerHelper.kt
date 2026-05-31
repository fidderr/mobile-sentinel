package com.mobilesentinel

import android.content.Intent
import android.util.Log

/**
 * Kotlin helper for barcode/QR scanning via the embedded ZXing scanner.
 *
 * Flow (cross-process, file-based — works without `onActivityResult`):
 * 1. Rust calls [scanFromAppContext] on a background thread (via JNI from
 *    `crate::features::camera`).
 * 2. This helper launches [SentinelScannerActivity].
 * 3. The user scans; the activity writes the decoded value to
 *    `<filesDir>/sentinel_scan_result.txt`.
 * 4. This helper polls for that file and returns its contents to Rust,
 *    blocking the JNI call until the scan completes or times out.
 */
object SentinelScannerHelper {

    private const val TAG = "MobileSentinel.Scanner"

    /**
     * Launch the barcode scanner using the app context (no Activity needed).
     * Blocks the calling thread until the scan completes or times out.
     * MUST be called from a background thread.
     */
    @JvmStatic
    fun scanFromAppContext(timeoutSeconds: Long = 60): String {
        val ctx = SentinelPrimitives.getAppContext()
            ?: run {
                Log.e(TAG, "scanFromAppContext: no appContext")
                return ""
            }

        // Notify kiosk BEFORE launching scanner so it won't relaunch main activity.
        SentinelActivityTracker.suppressRelaunchFor(2000)

        val resultFile = java.io.File(ctx.filesDir, "sentinel_scan_result.txt")
        resultFile.delete()

        // Launch scanner. Prefer the current activity (same task) over appContext
        // (which requires FLAG_ACTIVITY_NEW_TASK and creates a separate task).
        val activity = SentinelActivityTracker.currentResumedActivity
        try {
            if (activity != null) {
                val intent = Intent(activity, SentinelScannerActivity::class.java)
                activity.runOnUiThread {
                    activity.startActivity(intent)
                }
                Log.i(TAG, "Scanner activity launched from current activity (same task)")
            } else {
                val intent = Intent(ctx, SentinelScannerActivity::class.java)
                intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                ctx.startActivity(intent)
                Log.i(TAG, "Scanner activity launched from appContext (no activity available)")
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to launch scanner: ${e.message}", e)
            return ""
        }

        // Poll for the result file.
        val startMs = System.currentTimeMillis()
        val timeoutMs = timeoutSeconds * 1000
        while (System.currentTimeMillis() - startMs < timeoutMs) {
            if (resultFile.exists()) {
                val content = resultFile.readText().trim()
                resultFile.delete()
                Log.i(
                    TAG,
                    "Scan result: ${if (content.isNotEmpty()) content.take(20) + "..." else "(empty)"}",
                )
                return content
            }
            Thread.sleep(100)
        }

        Log.w(TAG, "Scan timed out after ${timeoutSeconds}s")
        return ""
    }
}
