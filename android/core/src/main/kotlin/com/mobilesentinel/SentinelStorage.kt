package com.mobilesentinel

import android.content.Context
import android.content.pm.PackageManager
import android.util.Log
import java.io.File

/**
 * Where this library keeps its files. An app built with `direct_boot = true` in
 * `sentinel.toml` keeps them in device-encrypted storage, which is open from boot,
 * so an alarm can ring before the first unlock. The manifest's own setting for this
 * is ignored for anything but system apps, so it has to be done here.
 */
object SentinelStorage {
    private const val TAG = "MobileSentinel.Storage"

    /** Written into the manifest by `build_sentinel`. */
    private const val META_DIRECT_BOOT = "com.mobilesentinel.direct_boot"

    @Volatile
    private var directBoot: Boolean? = null

    @JvmStatic
    fun isDirectBoot(context: Context): Boolean {
        directBoot?.let { return it }
        val app = context.applicationContext ?: context
        val on = try {
            app.packageManager
                .getApplicationInfo(app.packageName, PackageManager.GET_META_DATA)
                .metaData?.getBoolean(META_DIRECT_BOOT, false) ?: false
        } catch (e: Throwable) {
            Log.w(TAG, "reading the manifest failed: ${e.message}")
            false
        }
        directBoot = on
        return on
    }

    /** This library's files directory, the same one from every process. */
    @JvmStatic
    fun filesDir(context: Context): File {
        val app = context.applicationContext ?: context
        return if (isDirectBoot(app)) {
            app.createDeviceProtectedStorageContext().filesDir
        } else {
            app.filesDir
        }
    }
}
