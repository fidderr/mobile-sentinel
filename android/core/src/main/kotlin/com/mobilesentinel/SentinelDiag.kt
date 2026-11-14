package com.mobilesentinel

import android.content.Context
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Appends one line to the same rotating log as Rust's `diag::log` (`<files>/sentinel/diag.log`,
 * `diag.1.log`, 256 KB each), for events only Kotlin sees, such as why the app was woken.
 * Best effort: never throws.
 */
object SentinelDiag {
    private const val MAX_BYTES = 256L * 1024

    @JvmStatic
    fun log(context: Context, tag: String, message: String) {
        try {
            val dir = File(SentinelStorage.filesDir(context), "sentinel")
            dir.mkdirs()
            val current = File(dir, "diag.log")
            synchronized(this) {
                if (current.length() + message.length + 64 > MAX_BYTES && current.length() > 0) {
                    val previous = File(dir, "diag.1.log")
                    previous.delete()
                    current.renameTo(previous)
                }
                val stamp = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss.SSSXXX", Locale.ROOT).format(Date())
                current.appendText("$stamp $tag ${message.replace('\n', ' ')}\n")
            }
        } catch (_: Throwable) {
        }
    }
}
