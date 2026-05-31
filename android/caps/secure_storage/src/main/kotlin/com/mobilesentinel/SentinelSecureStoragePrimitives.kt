package com.mobilesentinel

import android.content.Context
import android.content.SharedPreferences
import android.util.Log

/**
 * Key/value storage primitives for the universal SDK.
 *
 * Rust calls INTO these `@JvmStatic` methods via JNI. Backed by a private
 * [SharedPreferences] file (`"sentinel_secure_prefs"`), matching the Rust
 * raw-JNI reference in `secure_storage.rs`.
 *
 * NOTE: this is plain `MODE_PRIVATE` storage, not Keystore-encrypted.
 * mobile-sentinel deliberately avoids the `androidx.security:security-crypto`
 * dependency to stay dependency-light; a consumer that needs true at-rest
 * encryption can layer it on top before calling [set]. The file is still
 * private to the app sandbox.
 *
 * Contract: no method ever throws. Each wraps its body in try/catch, logs a
 * warning, and returns the safe fallback (`false` / `null`).
 */
object SentinelSecureStoragePrimitives {
    private const val TAG = "MobileSentinel.SecureStorage"

    /** Preferences file name — shared with the Rust raw-JNI backend. */
    private const val PREFS_NAME = "sentinel_secure_prefs"

    /** Cached preferences instance, built once on first use. */
    @Volatile
    private var cached: SharedPreferences? = null

    /** Resolve (and cache) the backing private [SharedPreferences]. */
    private fun prefs(context: Context): SharedPreferences {
        cached?.let { return it }
        synchronized(this) {
            cached?.let { return it }
            val resolved = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            cached = resolved
            return resolved
        }
    }

    @JvmStatic
    fun set(key: String, value: String): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            prefs(context).edit().putString(key, value).apply()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "set($key): ${e.message}")
            false
        }
    }

    @JvmStatic
    fun get(key: String): String? {
        val context = SentinelPrimitives.getAppContext() ?: return null
        return try {
            prefs(context).getString(key, null)
        } catch (e: Throwable) {
            Log.w(TAG, "get($key): ${e.message}")
            null
        }
    }

    @JvmStatic
    fun delete(key: String): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            prefs(context).edit().remove(key).apply()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "delete($key): ${e.message}")
            false
        }
    }

    @JvmStatic
    fun clear(): Boolean {
        val context = SentinelPrimitives.getAppContext() ?: return false
        return try {
            prefs(context).edit().clear().apply()
            true
        } catch (e: Throwable) {
            Log.w(TAG, "clear(): ${e.message}")
            false
        }
    }
}
