package com.mobilesentinel

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import android.util.Log
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
    const val ACTION_JOB_HEADS_UP = SentinelActiveJobs.ACTION_JOB_HEADS_UP
    const val EXTRA_JOB_ID = SentinelActiveJobs.EXTRA_JOB_ID

    private val guarding = AtomicBoolean(false)
    private var guardThread: Thread? = null
    private val headsUpSent = mutableSetOf<String>()

    // All policy (how long between launches, how long a UI may be silent, how
    // far the allowance grows after a kill) comes from the job config that
    // Rust writes (`JobConfig`); the guardian only enforces it. A launch
    // throttle matters because every launch delivers a new intent that pauses
    // and resumes the activity, cancelling touches.
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
                val activeJobs = SentinelActiveJobs.list(context)
                if (activeJobs.isEmpty()) {
                    Log.i(TAG, "pollLoop: no active jobs — stopping")
                    livenessKills = 0
                    guarding.set(false)
                    break
                }

                val mainPid = SentinelRing.mainPid(context)
                val mainAlive = mainPid != 0

                for (job in activeJobs) {
                    val jobId = job.id
                    val config = job.json.optJSONObject("config")
                    if (!mainAlive) {
                        // MAIN is dead — start it. Clear heads-up tracking
                        // so we re-send after it comes back up.
                        headsUpSent.remove(jobId)
                        val now = SystemClock.elapsedRealtime()
                        val minLaunchGap = config?.optLong("min_launch_interval_ms", 3_000L) ?: 3_000L
                        if (lastLaunchAtMs != 0L && now - lastLaunchAtMs < minLaunchGap) {
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
                        // silence, up to the configured cap, so a UI that needs
                        // long to start is not killed in a loop.
                        val base = config?.optLong("ui_liveness_timeout_ms", 0) ?: 0
                        if (base > 0) {
                            val cap = (config?.optLong("ui_liveness_backoff_cap", 4L) ?: 4L).coerceAtLeast(1L)
                            val timeout = base * minOf(1L shl livenessKills.coerceAtMost(30), cap)
                            if (isUiFrozen(context, mainPid, job.sinceMs, timeout)) {
                                SentinelRing.noteFrozenKill(context)
                                killFrozenMain(mainPid, jobId, timeout)
                                break
                            }
                        }
                    }
                }

                // Sleep for the poll interval of the first active job
                // (all jobs in a single guardian share the shortest interval).
                val interval = activeJobs
                    .mapNotNull { it.json.optJSONObject("config")?.optLong("poll_interval_ms", 500) }
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
    private fun readActivityFqcn(context: Context): String? =
        SentinelActiveJobs.list(context).firstNotNullOfOrNull(SentinelActiveJobs::activityOf)
}
