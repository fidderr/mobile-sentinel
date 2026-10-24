package com.mobilesentinel

import android.os.SystemClock
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
}
