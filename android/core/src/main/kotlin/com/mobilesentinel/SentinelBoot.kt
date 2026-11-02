package com.mobilesentinel

import android.content.Context
import android.os.SystemClock
import android.os.UserManager
import java.io.File

/**
 * When the device booted, for telling state written before the current boot
 * (stale: every process, sound and lock-screen takeover died with it) from
 * state written since.
 *
 * Kotlin half of a rule that also lives in Rust (`utilities::boot`): state
 * written before the current boot is stale. It is needed here because the
 * kiosk lock and the job guardian act before any Rust has started; Rust
 * applies the same rule to its ring records and cleans up what this ignores.
 * Same boot-time formula on both sides: change one, change the other.
 */
object SentinelBoot {
    /** Wall-clock time (ms) of the current boot; `elapsedRealtime` counts through deep sleep. */
    @JvmStatic
    fun bootTimeMs(): Long = System.currentTimeMillis() - SystemClock.elapsedRealtime()

    /** True when [file] was last written before the current boot. */
    @JvmStatic
    fun isFromBeforeBoot(file: File): Boolean = file.lastModified() < bootTimeMs()

    /** False from a boot until the first unlock: credential-encrypted storage is closed then. */
    @JvmStatic
    fun isUserUnlocked(context: Context): Boolean =
        context.getSystemService(UserManager::class.java)?.isUserUnlocked ?: true

    /**
     * Whether this library's files can be read. Before the first unlock that takes
     * an app built with `direct_boot = true`; see [SentinelStorage].
     */
    @JvmStatic
    fun filesAvailable(context: Context): Boolean =
        isUserUnlocked(context) || SentinelStorage.isDirectBoot(context)
}
