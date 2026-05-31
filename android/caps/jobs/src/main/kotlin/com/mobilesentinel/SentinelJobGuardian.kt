package com.mobilesentinel

import android.app.ActivityManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Process
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
    const val ACTION_JOB_HEADS_UP = "com.mobilesentinel.JOB_HEADS_UP"
    const val EXTRA_JOB_ID = "job_id"

    private val guarding = AtomicBoolean(false)
    private var guardThread: Thread? = null
    private val headsUpSent = mutableSetOf<String>()

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
                    guarding.set(false)
                    break
                }

                val mainAlive = isMainProcessAlive(context)

                for (job in activeJobs) {
                    val jobId = job.optString("id", "")
                    val config = job.optJSONObject("config")
                    if (!mainAlive) {
                        // MAIN is dead — start it. Clear heads-up tracking
                        // so we re-send after it comes back up.
                        headsUpSent.remove(jobId)
                        val startDelay = config?.optLong("start_main_delay_ms", 0) ?: 0
                        if (startDelay > 0) {
                            Thread.sleep(startDelay)
                        }
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
                val json = JSONObject(file.readText())
                if (json.optString("status") == "active") {
                    active.add(json)
                }
            } catch (_: Throwable) {
                // Skip malformed files
            }
        }
        return active
    }

    private fun isMainProcessAlive(context: Context): Boolean {
        try {
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager
                ?: return false
            val myPid = Process.myPid()
            val processes = am.runningAppProcesses ?: return false
            for (proc in processes) {
                if (proc.pid == myPid) continue // skip :sentinel itself
                if (proc.processName == context.packageName) {
                    return true
                }
            }
        } catch (e: Throwable) {
            Log.w(TAG, "isMainProcessAlive check failed: ${e.message}")
        }
        return false
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
                    val json = JSONObject(file.readText())
                    if (json.optString("status") == "active") {
                        val payload = json.optJSONObject("payload")
                        val fqcn = payload?.optString("activity_fqcn", "")
                        if (!fqcn.isNullOrBlank()) return fqcn
                    }
                }
            }
        } catch (_: Throwable) {}
        return null
    }
}
