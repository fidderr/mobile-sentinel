package com.mobilesentinel

import android.content.Context
import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Key/value storage primitives for the universal SDK.
 *
 * Rust calls INTO these `@JvmStatic` methods via JNI. Backed by a private
 * [SharedPreferences] file (`"sentinel_secure_prefs"`), matching the Rust
 * raw-JNI reference in `secure_storage.rs`.
 *
 * NOTE: [set] and [get] are plain `MODE_PRIVATE` storage, not Keystore-encrypted.
 * mobile-sentinel deliberately avoids the `androidx.security:security-crypto`
 * dependency to stay dependency-light. For true at-rest encryption, [wrap] and
 * [unwrap] seal a secret with an AES key that lives in the Android Keystore and
 * never leaves it.
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

    private const val KEY_ALIAS = "sentinel_wrap_key"
    private const val KEYSTORE = "AndroidKeyStore"
    private const val CIPHER = "AES/GCM/NoPadding"
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128

    private fun wrapKey(): SecretKey {
        val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    /** Seals [secret] with the Keystore's key: iv, then ciphertext. `null` if it cannot. */
    @JvmStatic
    fun wrap(secret: ByteArray): ByteArray? {
        return try {
            val cipher = Cipher.getInstance(CIPHER)
            cipher.init(Cipher.ENCRYPT_MODE, wrapKey())
            cipher.iv + cipher.doFinal(secret)
        } catch (e: Throwable) {
            Log.w(TAG, "wrap: ${e.message}", e)
            null
        }
    }

    /** Opens what [wrap] sealed. `null` if it cannot: another device, or another install. */
    @JvmStatic
    fun unwrap(wrapped: ByteArray): ByteArray? {
        return try {
            val cipher = Cipher.getInstance(CIPHER)
            val spec = GCMParameterSpec(TAG_BITS, wrapped, 0, IV_BYTES)
            cipher.init(Cipher.DECRYPT_MODE, wrapKey(), spec)
            cipher.doFinal(wrapped, IV_BYTES, wrapped.size - IV_BYTES)
        } catch (e: Throwable) {
            Log.w(TAG, "unwrap: ${e.message}", e)
            null
        }
    }
}
