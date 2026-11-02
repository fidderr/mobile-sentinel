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
            val dir = File(context.filesDir, DIR)
            if (!File(dir, "$id.json").exists()) {
                Log.w(TAG, "mark: no job file for '$id'")
                return false
            }
            // A new fire makes a limit reached earlier irrelevant, whether or not
            // the app was around to deal with it.
            File(dir, "$id$LIMIT_SUFFIX").delete()
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
        val marker = File(File(context.filesDir, DIR), "$id$SUFFIX")
        return marker.exists() && !SentinelBoot.isFromBeforeBoot(marker)
    }

    /**
     * Leave word that [id]'s ring stopped at its limit, for Rust to end the alarm:
     * right away if MAIN runs, which is told, and otherwise when the guardian has
     * brought it up.
     */
    @JvmStatic
    fun markLimitReached(context: Context, id: String) {
        try {
            File(File(context.filesDir, DIR), "$id$LIMIT_SUFFIX").writeText("")
            context.sendBroadcast(
                Intent(ACTION_JOB_HEADS_UP).setPackage(context.packageName).putExtra(EXTRA_JOB_ID, id),
            )
        } catch (e: Throwable) {
            Log.w(TAG, "markLimitReached('$id') failed: ${e.message}")
        }
    }

    /**
     * Every active job with a readable job file. A marker from before the
     * current boot belongs to a session the reboot ended; MAIN's startup deals
     * with those, so they are left out here.
     */
    @JvmStatic
    fun list(context: Context): List<ActiveJob> {
        val dir = File(context.filesDir, DIR)
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
