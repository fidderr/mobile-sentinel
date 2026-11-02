package com.mobilesentinel

import android.content.Context
import android.content.Intent
import android.util.Log
import org.json.JSONObject
import java.io.File

/**
 * Which jobs must run right now, as seen from any process.
 *
 * A job is active while `sentinel/jobs/<id>.active` exists. Kotlin only creates
 * that marker; the job file itself (`<id>.json`) is written by Rust alone, and
 * Rust removes the marker. With one writer per file, the app saving a job while
 * it fires cannot lose the fire. Mirrors `ACTIVE_SUFFIX` in Rust (`features::jobs`).
 *
 * Lives in core because the alarm receiver, the guardian and the foreground
 * service all need it and none of them depends on another.
 */
object SentinelActiveJobs {
    private const val TAG = "MobileSentinel.Jobs"
    private const val DIR = "sentinel/jobs"
    private const val SUFFIX = ".active"

    /** A ring stopped at its limit. Mirrors `LIMIT_SUFFIX` in Rust; Rust removes it. */
    private const val LIMIT_SUFFIX = ".limit"

    /** The backup screen's challenge was solved. Mirrors `DISMISS_SUFFIX` in Rust; Rust removes it. */
    private const val DISMISS_SUFFIX = ".dismiss"

    const val ACTION_JOB_HEADS_UP = "com.mobilesentinel.JOB_HEADS_UP"
    const val EXTRA_JOB_ID = "job_id"

    class ActiveJob(val id: String, val json: JSONObject, val sinceMs: Long)

    /**
     * Create [id]'s marker, keeping a current one (and so its time). A marker
     * from before the boot belongs to a session the reboot ended, not to this
     * fire. Returns whether the job is active now: without a job file the alarm
     * no longer exists.
     */
    @JvmStatic
    fun mark(context: Context, id: String): Boolean {
        return try {
            val dir = File(SentinelStorage.filesDir(context), DIR)
            if (!File(dir, "$id.json").exists()) {
                Log.w(TAG, "mark: no job file for '$id'")
                return false
            }
            // A new fire makes how the previous ring ended irrelevant, whether or
            // not the app was around to deal with it.
            File(dir, "$id$LIMIT_SUFFIX").delete()
            File(dir, "$id$DISMISS_SUFFIX").delete()
            val marker = File(dir, "$id$SUFFIX")
            if (!marker.exists() || SentinelBoot.isFromBeforeBoot(marker)) {
                marker.writeText(System.currentTimeMillis().toString())
            }
            true
        } catch (e: Throwable) {
            Log.w(TAG, "mark('$id') failed: ${e.message}")
            false
        }
    }

    /** Whether [id] has a marker from this boot. */
    @JvmStatic
    fun isActive(context: Context, id: String): Boolean {
        val marker = File(File(SentinelStorage.filesDir(context), DIR), "$id$SUFFIX")
        return marker.exists() && !SentinelBoot.isFromBeforeBoot(marker)
    }

    /**
     * Leave word that [id]'s ring stopped at its limit, for Rust to end the alarm:
     * right away if MAIN runs, which is told, and otherwise when the guardian has
     * brought it up.
     */
    @JvmStatic
    fun markLimitReached(context: Context, id: String) = leaveWord(context, id, LIMIT_SUFFIX)

    /** The same for the backup screen's challenge having been solved. */
    @JvmStatic
    fun requestDismiss(context: Context, id: String) = leaveWord(context, id, DISMISS_SUFFIX)

    private fun leaveWord(context: Context, id: String, suffix: String) {
        try {
            File(File(SentinelStorage.filesDir(context), DIR), "$id$suffix").writeText("")
            context.sendBroadcast(
                Intent(ACTION_JOB_HEADS_UP).setPackage(context.packageName).putExtra(EXTRA_JOB_ID, id),
            )
        } catch (e: Throwable) {
            Log.w(TAG, "leaving word $suffix for '$id' failed: ${e.message}")
        }
    }

    /**
     * Every active job with a readable job file. A marker from before the
     * current boot belongs to a session the reboot ended; MAIN's startup deals
     * with those, so they are left out here.
     */
    @JvmStatic
    fun list(context: Context): List<ActiveJob> {
        val dir = File(SentinelStorage.filesDir(context), DIR)
        val markers = dir.listFiles { f -> f.name.endsWith(SUFFIX) } ?: return emptyList()
        val active = ArrayList<ActiveJob>(markers.size)
        for (marker in markers) {
            try {
                if (SentinelBoot.isFromBeforeBoot(marker)) continue
                val id = marker.name.removeSuffix(SUFFIX)
                val json = JSONObject(File(dir, "$id.json").readText())
                active.add(ActiveJob(id, json, marker.lastModified()))
            } catch (_: Throwable) {
                // One unreadable job must not hide the others.
            }
        }
        return active
    }

    /** The activity to bring up for [job], from its payload, or null. */
    @JvmStatic
    fun activityOf(job: ActiveJob): String? =
        job.json.optJSONObject("payload")?.optString("activity_fqcn", "")?.takeIf { it.isNotBlank() }
}
