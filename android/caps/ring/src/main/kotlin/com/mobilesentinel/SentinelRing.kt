package com.mobilesentinel

import android.app.ActivityManager
import android.app.AlarmManager
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.media.AudioManager
import android.os.Build
import android.os.PowerManager
import android.util.Log
import org.json.JSONObject
import java.io.File
import java.net.URLDecoder
import java.net.URLEncoder

/**
 * What is ringing, as seen from any process, and how to command the ring
 * service in `:sentinel`.
 *
 * Rust resolves everything about a ring (sound, volume, texts, how long it may
 * last) into a plan when it arms the alarm, because no Rust runs at ring time.
 * The plan travels with the OS alarm and is kept here while the alarm rings, so
 * a killed service picks up where it was. Kotlin only executes it.
 *
 * A ring exists only while its job's active marker does ([SentinelActiveJobs]).
 * Rust removes the marker when the alarm is dismissed or snoozed, so even a
 * lost stop command cannot leave a sound nobody can end.
 *
 * A module of its own because the alarm receiver, the ring service and the firing
 * sink all need it and none of them depends on another.
 */
object SentinelRing {
    private const val TAG = "MobileSentinel.Ring"
    private const val DIR = "sentinel/ringing"
    private const val PLAN_EXT = ".plan"
    private const val VOLUME_FILE = "volume_to_restore"
    private const val FROZEN_KILL_FILE = "frozen_kill"
    private const val UI_FAILED_FILE = "ui_failed"

    /** Read by `AlarmKit::timing_log`. */
    private const val TIMING_FILE = "timing.log"
    private const val TIMING_KEEP_BYTES = 32_000
    private const val ACTIVITY_THROTTLE_MS = 30_000L

    /** Touched when the user does something. Mirrors `ACTIVITY_FILE` in Rust. */
    private const val ACTIVITY_FILE = "activity"

    private const val SERVICE = "com.mobilesentinel.SentinelForegroundService"
    private const val RECEIVER = "com.mobilesentinel.SentinelAlarmReceiver"

    const val ACTION_RING = "com.mobilesentinel.ring.RING"
    const val ACTION_STOP = "com.mobilesentinel.ring.STOP"
    const val ACTION_PAUSE = "com.mobilesentinel.ring.PAUSE"
    const val ACTION_RESUME = "com.mobilesentinel.ring.RESUME"
    const val EXTRA_ID = "id"
    const val EXTRA_UI_VISIBLE = "ui_visible"

    /** Mirrors the receiver's `ACTION` and `EXTRA_INSTANCE_ID`. */
    private const val ALARM_ACTION = "com.mobilesentinel.ALARM_DISPATCH"
    private const val ALARM_EXTRA_ID = "instance_id"
    const val WATCHDOG_ID = "ring#watchdog"
    private const val WATCHDOG_REQUEST_CODE = 0x52494E47
    private const val WATCHDOG_LEAD_MS = 6_000L

    class Ringing(val id: String, val plan: JSONObject, val sinceMs: Long)

    private fun dir(context: Context) = File(SentinelStorage.filesDir(context), DIR)

    private fun planFile(context: Context, id: String) =
        File(dir(context), URLEncoder.encode(id, "UTF-8") + PLAN_EXT)

    private fun writeAtomically(file: File, text: String) {
        file.parentFile?.mkdirs()
        val tmp = File(file.parentFile, "${file.name}.${android.os.Process.myPid()}.tmp")
        tmp.writeText(text)
        java.nio.file.Files.move(
            tmp.toPath(),
            file.toPath(),
            java.nio.file.StandardCopyOption.ATOMIC_MOVE,
            java.nio.file.StandardCopyOption.REPLACE_EXISTING,
        )
    }

    // ---- Commands -----------------------------------------------------------

    /**
     * JNI entry: the firing sink (MAIN) engages a ring. The app coming back counts
     * as the user being there, unless it is back because the guardian killed a
     * frozen UI: that would otherwise keep an alarm nobody can solve ringing forever.
     */
    @JvmStatic
    fun startRing(id: String, planJson: String, uiVisible: Boolean): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        if (!File(dir(c), FROZEN_KILL_FILE).delete()) noteActivity(c)
        return start(c, id, planJson, uiVisible)
    }

    /** JNI entry: the firing sink (MAIN) ends a ring. */
    @JvmStatic
    fun stopRing(id: String): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return stop(c, id)
    }

    /** End [id]'s ring. Does nothing when it does not ring. */
    @JvmStatic
    fun stop(context: Context, id: String): Boolean {
        if (!planFile(context, id).exists()) return true
        return send(context, Intent(ACTION_STOP).putExtra(EXTRA_ID, id))
    }

    /** JNI entry: silence the ring while the user works on it. */
    @JvmStatic
    fun pauseRing(): Boolean {
        val c = SentinelPrimitives.getAppContext() ?: return false
        return send(c, Intent(ACTION_PAUSE))
    }

    /**
     * Keep the plan and bring the service up. The plan goes to disk first, so a
     * service that dies or is refused at this point is retried by the watchdog.
     * The file's time stays when the ring started.
     */
    @JvmStatic
    fun start(context: Context, id: String, planJson: String, uiVisible: Boolean = false): Boolean {
        try {
            val file = planFile(context, id)
            val startedAt = file.lastModified()
            writeAtomically(file, planJson)
            if (startedAt != 0L) file.setLastModified(startedAt)
        } catch (e: Throwable) {
            Log.w(TAG, "saving the plan of $id failed: ${e.message}")
        }
        return launch(context, Intent(ACTION_RING).putExtra(EXTRA_ID, id).putExtra(EXTRA_UI_VISIBLE, uiVisible))
    }

    /** Bring the service back for whatever is still ringing. Does nothing when nothing is. */
    @JvmStatic
    fun resumeSaved(context: Context): Boolean {
        if (list(context).isEmpty()) return false
        return launch(context, Intent(ACTION_RESUME))
    }

    private fun launch(context: Context, intent: Intent): Boolean {
        intent.setClassName(context.packageName, SERVICE)
        return try {
            val started = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
            started != null
        } catch (e: IllegalStateException) {
            // In the background without an exemption. The service may already
            // run, in which case it still takes commands.
            Log.w(TAG, "foreground start refused: ${e.message}")
            send(context, intent)
        } catch (e: Throwable) {
            Log.e(TAG, "starting the ring service failed: ${e.message}", e)
            false
        }
    }

    private fun send(context: Context, intent: Intent): Boolean {
        intent.setClassName(context.packageName, SERVICE)
        return try {
            context.startService(intent) != null
        } catch (e: Throwable) {
            Log.w(TAG, "command ${intent.action} not delivered: ${e.message}")
            false
        }
    }

    // ---- What is ringing ----------------------------------------------------

    /** Every ring whose job is still active, oldest first. The others are removed. */
    @JvmStatic
    fun list(context: Context): List<Ringing> {
        val files = dir(context).listFiles { f -> f.name.endsWith(PLAN_EXT) } ?: return emptyList()
        val out = ArrayList<Ringing>(files.size)
        for (file in files.sortedBy { it.lastModified() }) {
            val id = URLDecoder.decode(file.name.removeSuffix(PLAN_EXT), "UTF-8")
            if (!SentinelActiveJobs.isActive(context, id)) {
                file.delete()
                continue
            }
            // An unreadable plan still rings, with the defaults.
            val plan = try {
                JSONObject(file.readText())
            } catch (_: Throwable) {
                JSONObject()
            }
            out.add(Ringing(id, plan, file.lastModified()))
        }
        return out
    }

    @JvmStatic
    fun forget(context: Context, id: String) {
        planFile(context, id).delete()
    }

    /** When the user last did something in the app, 0 when never. */
    @JvmStatic
    fun lastActivityMs(context: Context): Long = File(dir(context), ACTIVITY_FILE).lastModified()

    /** The user is at the phone: a ring limit counts from now. Cheap and throttled. */
    @JvmStatic
    fun noteActivity(context: Context) {
        try {
            val file = File(dir(context), ACTIVITY_FILE)
            if (System.currentTimeMillis() - file.lastModified() < ACTIVITY_THROTTLE_MS) return
            file.parentFile?.mkdirs()
            file.writeText("")
        } catch (e: Throwable) {
            Log.w(TAG, "noteActivity failed: ${e.message}")
        }
    }

    /**
     * One line in the record of how fast rings were: when an alarm was delivered, the
     * service came up, the sound started. It is how a problem on a phone nobody here
     * has gets found. Kept small: the oldest half goes when it is full.
     */
    @JvmStatic
    fun logTiming(context: Context, event: String, id: String) {
        try {
            val file = File(dir(context), TIMING_FILE)
            file.parentFile?.mkdirs()
            if (file.length() > TIMING_KEEP_BYTES) {
                val text = file.readText()
                writeAtomically(file, text.substring(text.indexOf('\n', text.length / 2) + 1))
            }
            file.appendText("${System.currentTimeMillis()} $event $id\n")
        } catch (e: Throwable) {
            Log.w(TAG, "logTiming failed: ${e.message}")
        }
    }

    /** The UI cannot be relied on for this ring: the backup screen takes over. */
    @JvmStatic
    fun markUiFailed(context: Context) {
        try {
            val file = File(dir(context), UI_FAILED_FILE)
            file.parentFile?.mkdirs()
            file.writeText("")
        } catch (e: Throwable) {
            Log.w(TAG, "markUiFailed failed: ${e.message}")
        }
    }

    @JvmStatic
    fun uiFailed(context: Context): Boolean {
        val file = File(dir(context), UI_FAILED_FILE)
        return file.exists() && !SentinelBoot.isFromBeforeBoot(file)
    }

    @JvmStatic
    fun clearUiFailed(context: Context) {
        File(dir(context), UI_FAILED_FILE).delete()
    }

    /** The guardian is about to kill a frozen UI; see [startRing]. */
    @JvmStatic
    fun noteFrozenKill(context: Context) {
        try {
            val file = File(dir(context), FROZEN_KILL_FILE)
            file.parentFile?.mkdirs()
            file.writeText("")
        } catch (e: Throwable) {
            Log.w(TAG, "noteFrozenKill failed: ${e.message}")
        }
    }

    // ---- MAIN, seen from :sentinel ------------------------------------------

    /** MAIN's pid, or 0 when it is not running. */
    @JvmStatic
    fun mainPid(context: Context): Int {
        try {
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager ?: return 0
            for (proc in am.runningAppProcesses ?: return 0) {
                if (proc.processName == context.packageName) return proc.pid
            }
        } catch (e: Throwable) {
            Log.w(TAG, "mainPid check failed: ${e.message}")
        }
        return 0
    }

    /** Whether MAIN's main activity is on screen right now. */
    @JvmStatic
    fun mainOnScreen(context: Context): Boolean {
        return try {
            val file = File(SentinelStorage.filesDir(context), SentinelActivityTracker.MAIN_RESUMED_FILE)
            if (!file.exists()) return false
            val pid = file.readText().trim().substringBefore(' ').toIntOrNull() ?: return false
            pid != 0 && pid == mainPid(context)
        } catch (_: Throwable) {
            false
        }
    }

    // ---- Keeping the CPU awake ----------------------------------------------

    private var cpu: PowerManager.WakeLock? = null

    /**
     * Keep the CPU awake for [ms], replacing an earlier hold. An alarm delivery
     * only guarantees that until the receiver returns, which is before the
     * service has started.
     */
    @JvmStatic
    @Synchronized
    fun holdCpu(context: Context, ms: Long) {
        try {
            val lock = cpu ?: (context.getSystemService(Context.POWER_SERVICE) as PowerManager)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "MobileSentinel:ring")
                .also {
                    it.setReferenceCounted(false)
                    cpu = it
                }
            lock.acquire(ms)
        } catch (e: Throwable) {
            Log.w(TAG, "holdCpu failed: ${e.message}")
        }
    }

    @JvmStatic
    @Synchronized
    fun releaseCpu() {
        try {
            cpu?.takeIf { it.isHeld }?.release()
        } catch (e: Throwable) {
            Log.w(TAG, "releaseCpu failed: ${e.message}")
        }
    }

    // ---- Alarm volume -------------------------------------------------------

    /**
     * Raise the alarm stream to at least [percent] of its range. The level from
     * before goes to disk the first time, so a killed process cannot leave the
     * phone loud. Does nothing for 0, or when it is already loud enough. Some
     * devices refuse this during Do Not Disturb; the alarm then plays at the
     * user's volume.
     */
    @JvmStatic
    fun holdVolume(context: Context, percent: Int) {
        if (percent <= 0) return
        try {
            val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            val max = am.getStreamMaxVolume(AudioManager.STREAM_ALARM)
            val target = Math.ceil(max * percent.coerceAtMost(100) / 100.0).toInt().coerceIn(1, max)
            val current = am.getStreamVolume(AudioManager.STREAM_ALARM)
            if (current >= target) return
            val saved = File(dir(context), VOLUME_FILE)
            if (!saved.exists()) writeAtomically(saved, current.toString())
            am.setStreamVolume(AudioManager.STREAM_ALARM, target, 0)
            Log.i(TAG, "alarm volume raised $current -> $target (of $max)")
        } catch (e: Throwable) {
            Log.w(TAG, "holdVolume: ${e.message}")
        }
    }

    /** Put the alarm stream back to where [holdVolume] found it. */
    @JvmStatic
    fun restoreVolume(context: Context) {
        val saved = File(dir(context), VOLUME_FILE)
        try {
            if (!saved.exists()) return
            val level = saved.readText().trim().toInt()
            val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            am.setStreamVolume(AudioManager.STREAM_ALARM, level, 0)
            Log.i(TAG, "alarm volume restored to $level")
        } catch (e: Throwable) {
            Log.w(TAG, "restoreVolume: ${e.message}")
        } finally {
            saved.delete()
        }
    }

    /** After a kill, a force-stop or a reboot: put back what nothing is ringing for any more. */
    @JvmStatic
    fun restoreVolumeIfIdle(context: Context) {
        if (list(context).isEmpty()) restoreVolume(context)
    }

    // ---- Watchdog -----------------------------------------------------------
    //
    // While something rings the service keeps an exact alarm armed a few seconds
    // ahead. Some OEM cleaners kill every process of the app at once; the
    // re-arming then stops, the alarm goes off, and AlarmManager, which no app
    // kill can touch, wakes the receiver to bring the ring back.

    private fun watchdogIntent(context: Context, flags: Int): PendingIntent? {
        val intent = Intent(ALARM_ACTION).apply {
            component = ComponentName(context.packageName, RECEIVER)
            addFlags(Intent.FLAG_RECEIVER_FOREGROUND)
            putExtra(ALARM_EXTRA_ID, WATCHDOG_ID)
        }
        return PendingIntent.getBroadcast(
            context,
            WATCHDOG_REQUEST_CODE,
            intent,
            flags or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    /** Arm the watchdog, or move it forward. Exact but not user-visible. */
    @JvmStatic
    fun armWatchdog(context: Context) {
        try {
            val am = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
            val pi = watchdogIntent(context, PendingIntent.FLAG_UPDATE_CURRENT) ?: return
            val at = System.currentTimeMillis() + WATCHDOG_LEAD_MS
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && !am.canScheduleExactAlarms()) {
                am.set(AlarmManager.RTC_WAKEUP, at, pi)
            } else {
                am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pi)
            }
        } catch (e: Throwable) {
            Log.w(TAG, "armWatchdog failed: ${e.message}")
        }
    }

    @JvmStatic
    fun cancelWatchdog(context: Context) {
        try {
            val am = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
            val pi = watchdogIntent(context, PendingIntent.FLAG_NO_CREATE) ?: return
            am.cancel(pi)
            pi.cancel()
        } catch (e: Throwable) {
            Log.w(TAG, "cancelWatchdog failed: ${e.message}")
        }
    }
}
