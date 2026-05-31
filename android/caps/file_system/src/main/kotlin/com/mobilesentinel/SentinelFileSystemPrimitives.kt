package com.mobilesentinel

import android.util.Log
import org.json.JSONArray
import java.io.File

/**
 * File-system capability — copy/list bundled APK assets.
 *
 * Lives in the `:sentinel-file_system` module, compiled only when the
 * `file_system` Cargo feature is enabled. The app context comes from
 * [SentinelPrimitives.getAppContext].
 *
 * Rust calls INTO these via JNI (`crate::features::file_system`).
 */
object SentinelFileSystemPrimitives {
    private const val TAG = "MobileSentinel.Fs"

    @JvmStatic
    fun copyAsset(assetPath: String, destPath: String): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return try {
            c.assets.open(assetPath).use { input ->
                val dest = File(destPath)
                dest.parentFile?.mkdirs()
                dest.outputStream().use { input.copyTo(it) }
            }
            true
        } catch (e: Throwable) {
            Log.e(TAG, "copyAsset($assetPath -> $destPath): ${e.message}", e)
            false
        }
    }

    @JvmStatic
    fun listAssets(path: String): String {
        val c = SentinelPrimitives.getAppContext() ?: return "[]"
        return try {
            val names = c.assets.list(path) ?: emptyArray()
            JSONArray(names.toList()).toString()
        } catch (e: Throwable) {
            Log.e(TAG, "listAssets($path): ${e.message}", e)
            "[]"
        }
    }
}
