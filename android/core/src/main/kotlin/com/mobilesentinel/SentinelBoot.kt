package com.mobilesentinel

import android.os.SystemClock
import java.io.File

/**
 * When the device booted, for telling state written before the current boot
 * (stale: every process, sound and lock-screen takeover died with it) from
 * state written since.
 */
object SentinelBoot {
    /** Wall-clock time (ms) of the current boot; `elapsedRealtime` counts through deep sleep. */
    @JvmStatic
    fun bootTimeMs(): Long = System.currentTimeMillis() - SystemClock.elapsedRealtime()

    /** True when [file] was last written before the current boot. */
    @JvmStatic
    fun isFromBeforeBoot(file: File): Boolean = file.lastModified() < bootTimeMs()
}
