package com.mobilesentinel

import android.app.ActivityManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import android.util.Log
import org.json.JSONObject
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Generic job guardian for the `:sentinel` process.
 *
 * This singleton knows nothing about alarms, audio, or kiosk. It only knows:
 * - "I have jobs that need MAIN alive"
 * - "Is MAIN alive? No → start it"
 * - "Is the job done? No → keep polling. Yes → stop polling"
 *
 * Completely reusable for any consumer (alarm apps, timer apps, reminder apps).
 * It runs in the `:sentinel` child process, which exists precisely so it can
 * survive when MAIN (Rust) is dead — so this logic is necessarily Kotlin-only.
 * It reads ONLY the job files (the contract Rust writes when alive); it does
 * not depend on kiosk or any other module.
 */
object SentinelJobGuardian {
    private const val TAG = "MobileSentinel.Guardian"
    private const val JOBS_DIR = "sentinel/jobs"
    /** Mirrors `UI_HEARTBEAT_FILE` in Rust (`features::jobs`). */
    private const val UI_HEARTBEAT_FILE = "ui_heartbeat"
    /** Written by core's `SentinelActivityTracker`. */
    private const val MAIN_RESUMED_FILE = SentinelActivityTracker.MAIN_RESUMED_FILE
    const val ACTION_JOB_HEADS_UP = "com.mobilesentinel.JOB_HEADS_UP"
    const val EXTRA_JOB_ID = "job_id"

    private val guarding = AtomicBoolean(false)
    private var guardThread: Thread? = null
    private val headsUpSent = mutableSetOf<String>()

    /**
     * Minimum gap between two MAIN launches. A cold start takes a few
     * seconds; launching again on every poll meanwhile delivers a new intent
     * each time, and every delivery pauses and resumes the activity, which
     * cancels the user's touches and eventually wedges the WebView.
     */
    private const val MIN_LAUNCH_INTERVAL_MS = 3_000L
    @Volatile private var lastLaunchAtMs = 0L

    /** MAIN kills for a frozen UI since the guardian last had nothing to guard. */
    @Volatile private var livenessKills = 0

    /**
     * Start the polling loop. Called when an alarm receiver fires and
     * activates a job. Safe to call multiple times — only one loop runs.
     */
    @JvmStatic
    fun startGuarding(context: Context) {
        if (guarding.getAndSet(true)) {
            Log.d(TAG, "startGuarding: already running")
            return
        }
        Log.i(TAG, "startGuarding: launching poll loop")
        val appContext = context.applicationContext
        guardThread = Thread({
            pollLoop(appContext)
        }, "SentinelJobGuardian").apply {
            isDaemon = true
            start()
        }
    }

    /**
     * Stop the polling loop. Called when no active jobs remain or
     * explicitly by the consumer.
     */
    @JvmStatic
    fun stopGuarding() {
        if (!guarding.getAndSet(false)) return
        Log.i(TAG, "stopGuarding: signalling loop to stop")
        guardThread?.interrupt()
        guardThread = null
        headsUpSent.clear()
    }

    /**
     * Clear heads-up tracking for a specific job. Called by the alarm
     * receiver on every fire/snooze-refire so the guardian re-sends
     * the broadcast even if it was already sent for a previous fire
     * of the same job.
     */
    @JvmStatic
    fun clearHeadsUpFor(jobId: String) {
        headsUpSent.remove(jobId)
    }

    private fun pollLoop(context: Context) {
        Log.i(TAG, "pollLoop: started")
        try {
            while (guarding.get()) {
                val activeJobs = readActiveJobs(context)
                if (activeJobs.isEmpty()) {
                    Log.i(TAG, "pollLoop: no active jobs — stopping")
                    livenessKills = 0
                    guarding.set(false)
                    break
                }

                val mainPid = mainPid(context)
                val mainAlive = mainPid != 0

                for (job in activeJobs) {
                    val jobId = job.optString("id", "")
                    val config = job.optJSONObject("config")
                    if (!mainAlive) {
                        // MAIN is dead — start it. Clear heads-up tracking
                        // so we re-send after it comes back up.
                        headsUpSent.remove(jobId)
                        val now = SystemClock.elapsedRealtime()
                        if (lastLaunchAtMs != 0L && now - lastLaunchAtMs < MIN_LAUNCH_INTERVAL_MS) {
                            // A launch is already on its way; give MAIN time to come up.
                            break
                        }
                        val startDelay = config?.optLong("start_main_delay_ms", 0) ?: 0
                        if (startDelay > 0) {
                            Thread.sleep(startDelay)
                        }
                        lastLaunchAtMs = SystemClock.elapsedRealtime()
                        startMainActivity(context)
                        // After starting MAIN, break and re-poll next iteration
                        // to give MAIN time to come up.
                        break
                    } else {
                        // MAIN is alive — send heads-up ONCE per job.
                        // Don't spam every poll cycle (causes audio restart).
                        if (!headsUpSent.contains(jobId)) {
                            val headsUpDelay = config?.optLong("heads_up_delay_ms", 200) ?: 200
                            if (headsUpDelay > 0) {
                                Thread.sleep(headsUpDelay)
                            }
                            sendHeadsUpBroadcast(context, jobId)
                            headsUpSent.add(jobId)
                        }
                        // Opt-in liveness check. Each kill doubles the allowed
                        // silence (up to 4x) so a UI that needs long to start
                        // is not killed in a loop.
                        val base = config?.optLong("ui_liveness_timeout_ms", 0) ?: 0
                        if (base > 0) {
                            val timeout = base shl livenessKills.coerceAtMost(2)
                            if (isUiFrozen(context, mainPid, job.optLong("_active_since_ms", 0L), timeout)) {
                                killFrozenMain(mainPid, jobId, timeout)
                                break
                            }
                        }
                    }
                }

                // Sleep for the poll interval of the first active job
                // (all jobs in a single guardian share the shortest interval).
                val interval = activeJobs
                    .mapNotNull { it.optJSONObject("config")?.optLong("poll_interval_ms", 500) }
                    .minOrNull() ?: 500L
                Thread.sleep(interval)
            }
        } catch (_: InterruptedException) {
            Log.i(TAG, "pollLoop: interrupted")
        } catch (e: Throwable) {
            Log.e(TAG, "pollLoop: unexpected error", e)
        } finally {
            guarding.set(false)
            Log.i(TAG, "pollLoop: exited")
        }
    }

    private fun readActiveJobs(context: Context): List<JSONObject> {
        val dir = File(context.filesDir, JOBS_DIR)
        if (!dir.isDirectory) return emptyList()
        val files = dir.listFiles { f -> f.extension == "json" } ?: return emptyList()
        val active = mutableListOf<JSONObject>()
        for (file in files) {
            try {
                // An "active" job last written before the current boot belongs
                // to a session the reboot ended; MAIN's startup cleans it up.
                if (SentinelBoot.isFromBeforeBoot(file)) continue
                val json = JSONObject(file.readText())
                if (json.optString("status") == "active") {
                    // The job file is rewritten when the job becomes active, so
                    // its mtime is when the guarded session began.
                    json.put("_active_since_ms", file.lastModified())
                    active.add(json)
                }
            } catch (_: Throwable) {
                // Skip malformed files
            }
        }
        return active
    }

    /**
     * MAIN is alive when a process named exactly `packageName` runs. That
     * includes this process when the guardian was started inside MAIN (e.g.
     * by a receiver without `android:process`); skipping our own pid there
     * made the guardian see MAIN as dead and relaunch it every poll.
     */
    private fun isMainProcessAlive(context: Context): Boolean = mainPid(context) != 0

    /** MAIN's pid, or 0 when it is not running. */
    private fun mainPid(context: Context): Int {
        try {
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager
                ?: return 0
            val processes = am.runningAppProcesses ?: return 0
            for (proc in processes) {
                if (proc.processName == context.packageName) {
                    return proc.pid
                }
            }
        } catch (e: Throwable) {
            Log.w(TAG, "mainPid check failed: ${e.message}")
        }
        return 0
    }

    /**
     * Whether MAIN's UI looks frozen: its main activity has been on screen
     * (`mobile_sentinel_main_resumed`, written by the core activity tracker)
     * for longer than [timeoutMs] without the UI reporting a heartbeat
     * (`sentinel/jobs/ui_heartbeat`, see `jobs::ui_heartbeat` in Rust). The
     * clock starts at the latest of the resume, the last heartbeat and the
     * moment the job became active (a UI idle for a while before the alarm
     * fired has not frozen). A UI
     * that is not on screen (screen off, scanner in front, no activity yet)
     * is never judged, and neither is a marker left by another process.
     */
    private fun isUiFrozen(context: Context, mainPid: Int, activeSinceMs: Long, timeoutMs: Long): Boolean {
        try {
            val marker = File(context.filesDir, MAIN_RESUMED_FILE)
            if (!marker.exists()) return false
            val parts = marker.readText().trim().split(' ')
            if (parts.size != 2 || parts[0].toIntOrNull() != mainPid) return false
            val resumedAt = parts[1].toLongOrNull() ?: return false
            val beatAt = File(context.filesDir, "$JOBS_DIR/$UI_HEARTBEAT_FILE").lastModified()
            val lastSign = maxOf(resumedAt, beatAt, activeSinceMs)
            return System.currentTimeMillis() - lastSign > timeoutMs
        } catch (e: Throwable) {
            Log.w(TAG, "isUiFrozen check failed: ${e.message}")
        }
        return false
    }

    /** Kill MAIN so the next poll relaunches it. */
    private fun killFrozenMain(mainPid: Int, jobId: String, timeoutMs: Long) {
        livenessKills++
        Log.w(TAG, "UI frozen for more than ${timeoutMs}ms — killing MAIN (pid $mainPid), kill #$livenessKills")
        headsUpSent.remove(jobId)
        android.os.Process.killProcess(mainPid)
    }

    private fun startMainActivity(context: Context) {
        val fqcn = readActivityFqcn(context)
        if (fqcn == null) {
            Log.w(TAG, "startMainActivity: no activity FQCN available")
            return
        }
        try {
            val intent = Intent().apply {
                component = ComponentName(context.packageName, fqcn)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or
                        Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or
                        Intent.FLAG_ACTIVITY_SINGLE_TOP
            }
            context.startActivity(intent)
            Log.i(TAG, "startMainActivity: launched $fqcn")
        } catch (e: Throwable) {
            Log.w(TAG, "startMainActivity failed: ${e.message}")
        }
    }

    private fun sendHeadsUpBroadcast(context: Context, jobId: String) {
        try {
            val intent = Intent(ACTION_JOB_HEADS_UP).apply {
                setPackage(context.packageName)
                putExtra(EXTRA_JOB_ID, jobId)
            }
            context.sendBroadcast(intent)
            Log.d(TAG, "sendHeadsUpBroadcast: job=$jobId")
        } catch (e: Throwable) {
            Log.w(TAG, "sendHeadsUpBroadcast failed: ${e.message}")
        }
    }

    /**
     * Read the activity FQCN from the active job payloads. The consumer puts
     * `activity_fqcn` into the job payload at registration time (when
     * MAIN/Rust is alive), so it is always present by the time a job fires.
     * Returns the first non-blank match, or null.
     *
     * The guardian deliberately reads ONLY the job payload — it does not
     * depend on kiosk state or any other module's files. Jobs is a standalone
     * feature: a consumer can enable `jobs` without `kiosk`.
     */
    private fun readActivityFqcn(context: Context): String? {
        try {
            val dir = File(context.filesDir, JOBS_DIR)
            if (dir.isDirectory) {
                val files = dir.listFiles { f -> f.extension == "json" } ?: emptyArray()
                for (file in files) {
                    // One unreadable job file must not hide the others.
                    try {
                        val json = JSONObject(file.readText())
                        if (json.optString("status") == "active") {
                            val payload = json.optJSONObject("payload")
                            val fqcn = payload?.optString("activity_fqcn", "")
                            if (!fqcn.isNullOrBlank()) return fqcn
                        }
                    } catch (_: Throwable) {}
                }
            }
        } catch (_: Throwable) {}
        return null
    }
}
