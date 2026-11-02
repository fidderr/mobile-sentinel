package com.mobilesentinel

import android.content.Context
import android.util.Log
import java.io.File
import java.net.URLDecoder
import java.net.URLEncoder

/**
 * What this module has armed with the OS, kept on disk so it can be armed
 * again after a reboot (Android clears every `AlarmManager` entry then).
 *
 * This is recovery of OS state, not a decision: Rust decides which alarms
 * exist and when they ring, the arming primitives record exactly what they
 * were asked to arm, and [SentinelBootService] replays that record. Nothing
 * here knows what an alarm *is* — only an id, a time and the plan to hand back —
 * and it does not read Rust's own files.
 *
 * One small file per alarm (`sentinel/armed/<id>.json`, the id URL-encoded),
 * so the MAIN process (arming) and `:sentinel` (forgetting an alarm that just
 * fired) never rewrite a shared file.
 */
internal object SentinelArmedAlarms {
    private const val TAG = "MobileSentinel.Armed"
    private const val DIR = "sentinel/armed"
    private const val EXT = ".armed"

    private fun dir(context: Context) = File(context.applicationContext.filesDir, DIR)

    private fun fileFor(context: Context, id: String) =
        File(dir(context), URLEncoder.encode(id, "UTF-8") + EXT)

    class Armed(val fireAtMs: Long, val metadataJson: String?)

    /** Remember that [id] is armed for [fireAtMs]. Internal watchdog alarms are not recorded. */
    fun record(context: Context, id: String, fireAtMs: Long, metadataJson: String?) {
        if (id.endsWith(SentinelAlarmReceiver.WATCHDOG_SUFFIX)) return
        try {
            val d = dir(context)
            d.mkdirs()
            val file = fileFor(context, id)
            val tmp = File(d, "${file.name}.${android.os.Process.myPid()}.tmp")
            tmp.writeText(fireAtMs.toString() + "\n" + metadataJson.orEmpty())
            java.nio.file.Files.move(
                tmp.toPath(),
                file.toPath(),
                java.nio.file.StandardCopyOption.ATOMIC_MOVE,
                java.nio.file.StandardCopyOption.REPLACE_EXISTING,
            )
        } catch (e: Throwable) {
            Log.w(TAG, "record($id) failed: ${e.message}")
        }
    }

    /** Forget [id]: it was cancelled, or it fired. */
    fun forget(context: Context, id: String) {
        try {
            fileFor(context, id).delete()
        } catch (e: Throwable) {
            Log.w(TAG, "forget($id) failed: ${e.message}")
        }
    }

    /** Every recorded alarm by id. Unreadable entries are dropped. */
    fun all(context: Context): Map<String, Armed> {
        val files = dir(context).listFiles { f -> f.name.endsWith(EXT) } ?: return emptyMap()
        val out = HashMap<String, Armed>()
        for (file in files) {
            try {
                val lines = file.readText().split('\n', limit = 2)
                val metadata = lines.getOrNull(1)?.takeIf { it.isNotBlank() }
                out[URLDecoder.decode(file.name.removeSuffix(EXT), "UTF-8")] =
                    Armed(lines[0].trim().toLong(), metadata)
            } catch (_: Throwable) {
                file.delete()
            }
        }
        return out
    }
}
