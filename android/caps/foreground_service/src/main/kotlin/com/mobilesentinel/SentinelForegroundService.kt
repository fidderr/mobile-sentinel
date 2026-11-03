package com.mobilesentinel

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ComponentName
import android.content.Intent
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject

/**
 * The ring: sound, vibration and the foreground notification of every alarm
 * that is ringing. Runs in `:sentinel`, the one process without the UI
 * framework, so nothing that happens to the UI can stop the sound, and it needs
 * no Rust: an alarm rings from the plan Rust wrote when it armed it (see
 * [SentinelRing]). All policy is in that plan; this service only executes it.
 *
 * Several alarms can ring at once. One of them owns the sound and vibration, and
 * when it ends the next one takes over. The service stops when none is left.
 */
class SentinelForegroundService : Service() {

    companion object {
        private const val TAG = "MobileSentinel.Service"
        private const val NOTIFICATION_ID = 9001
        private const val TICK_MS = 1_000L
        private const val WATCHDOG_EVERY_TICKS = 2
        private const val CPU_RENEW_EVERY_TICKS = 60
        /** The sound is on by then. */
        private const val WEBVIEW_CHECK_TICK = 2
        private const val CPU_HOLD_MS = 2 * 60 * 1000L
        /** Long enough for the system to hand over a command it is holding. */
        private const val RESTART_GRACE_MS = 300L
        private const val FALLBACK_CHANNEL = "sentinel_foreground"
        private const val DEFAULT_SOUND = "system://default-alarm"
        private const val SILENT_SOUND = "silent://"
        private val DEFAULT_VIBRATION = longArrayOf(0, 400, 200, 400)
    }

    private val handler = Handler(Looper.getMainLooper())
    private val ringing = LinkedHashMap<String, SentinelRing.Ringing>()

    /** The sound of a ring that vibrates first, until its time comes. */
    private var soundLater: Runnable? = null

    /** The ring whose sound and vibration are on. */
    private var owner: String? = null
    private var paused = false
    private var uiVisible = false
    private var postedFullScreen = false
    private var postedBackup = false
    private var postedOwner: String? = null
    private var backupLaunched = false
    private var commandSeen = false
    private var ticking = false
    private var ticks = 0

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        SentinelPrimitives.init(applicationContext)
        Log.i(TAG, "onCreate")
        // When Android restarts this sticky service after a kill, some OEMs (seen
        // on ColorOS) call onCreate but never deliver the null-intent
        // onStartCommand, so the ring comes back from here as well.
        handler.postDelayed({ if (!commandSeen) resumeSaved() }, RESTART_GRACE_MS)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        commandSeen = true
        when (intent?.action) {
            SentinelRing.ACTION_RING -> ring(
                intent.getStringExtra(SentinelRing.EXTRA_ID).orEmpty(),
                intent.getBooleanExtra(SentinelRing.EXTRA_UI_VISIBLE, false),
            )
            SentinelRing.ACTION_STOP -> stop(intent.getStringExtra(SentinelRing.EXTRA_ID).orEmpty())
            SentinelRing.ACTION_PAUSE -> pause()
            // RESUME, or null: Android restarted this sticky service after a kill.
            else -> resumeSaved()
        }
        return if (ringing.isEmpty()) START_NOT_STICKY else START_STICKY
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        stopOutputs()
        Log.i(TAG, "onDestroy")
        super.onDestroy()
    }

    // ---- Commands -----------------------------------------------------------

    private fun ring(id: String, visible: Boolean) {
        reload()
        uiVisible = visible || SentinelRing.mainOnScreen(this)
        val first = owner == null && id in ringing
        if (first) {
            owner = id
            SentinelRing.logTiming(this, "service", id)
        }
        // First, whatever fails later: Android kills a service started in the
        // foreground that does not get there in time.
        showForeground()
        if (id !in ringing) {
            Log.w(TAG, "RING for '$id', which has no active job")
            settle()
            return
        }
        val wasPaused = paused
        paused = false
        if (first) {
            startOutputs(ringing.getValue(id), fade = true)
        } else if (wasPaused) {
            ringing[owner]?.let { startOutputs(it, fade = false) }
        }
        settle()
    }

    private fun stop(id: String) {
        SentinelRing.forget(this, id)
        drop(id)
        // A service started just for this command has to find the others first.
        reload()
        settle()
    }

    private fun pause() {
        reload()
        showForeground()
        paused = true
        stopOutputs()
        settle()
    }

    private fun resumeSaved() {
        commandSeen = true
        reload()
        showForeground()
        if (owner == null) {
            ringing.values.firstOrNull()?.let { startOutputs(it, fade = false) }
        }
        settle()
    }

    private fun reload() {
        val saved = SentinelRing.list(this)
        ringing.keys.retainAll(saved.map { it.id }.toSet())
        for (ring in saved) ringing[ring.id] = ring
        if (owner !in ringing) owner = null
    }

    private fun drop(id: String) {
        ringing.remove(id)
        if (owner == id) {
            stopOutputs()
            owner = null
            paused = false
        }
    }

    /** After any change: hand the sound over, or finish when nothing is left. */
    private fun settle() {
        if (ringing.isEmpty()) {
            shutDown()
            return
        }
        if (owner == null && !paused) {
            ringing.values.firstOrNull()?.let { startOutputs(it, fade = true) }
        }
        showForeground()
        if (!ticking) {
            ticking = true
            SentinelRing.holdCpu(this, CPU_HOLD_MS)
            handler.post(tick)
        }
    }

    private fun shutDown() {
        Log.i(TAG, "nothing is ringing any more, stopping")
        handler.removeCallbacks(tick)
        ticking = false
        stopOutputs()
        SentinelRing.restoreVolume(this)
        SentinelRing.clearUiFailed(this)
        SentinelRing.cancelWatchdog(this)
        SentinelRing.releaseCpu()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
        stopSelf()
    }

    // ---- Once a second while ringing ----------------------------------------

    private val tick = object : Runnable {
        override fun run() {
            ticks++
            if (ticks == WEBVIEW_CHECK_TICK) SentinelWebViewCheck.refresh(this@SentinelForegroundService)
            // The marker is gone when Rust ended the alarm: never ring on without
            // it, even if the stop command got lost.
            for (id in ringing.keys.toList()) {
                if (!SentinelActiveJobs.isActive(this@SentinelForegroundService, id)) {
                    Log.i(TAG, "$id is no longer active, ending its ring")
                    SentinelRing.forget(this@SentinelForegroundService, id)
                    drop(id)
                }
            }
            endRingsPastTheirLimit()
            bringUpBackupScreenIfNeeded()
            if (ringing.isEmpty()) {
                shutDown()
                return
            }
            // Nobody left to bring a paused sound back.
            if (paused && SentinelRing.mainPid(this@SentinelForegroundService) == 0) {
                Log.i(TAG, "MAIN died during a pause, sound back on")
                paused = false
                ringing[owner]?.let { startOutputs(it, fade = false) }
            }
            if (!paused) {
                if (owner == null) {
                    ringing.values.firstOrNull()?.let { startOutputs(it, fade = false) }
                }
                ringing[owner]?.let {
                    SentinelRing.holdVolume(this@SentinelForegroundService, it.plan.optInt("min_volume_percent", 0))
                }
                SentinelFiringAudioPrimitives.checkPlayback()
            }
            if (ticks % WATCHDOG_EVERY_TICKS == 0) SentinelRing.armWatchdog(this@SentinelForegroundService)
            if (ticks % CPU_RENEW_EVERY_TICKS == 0) SentinelRing.holdCpu(this@SentinelForegroundService, CPU_HOLD_MS)
            handler.postDelayed(this, TICK_MS)
        }
    }

    /** Whether the app's own screen cannot be counted on for this ring. */
    private fun needsBackup(plan: JSONObject): Boolean =
        plan.optInt("backup_holds", 0) > 0 &&
            (!SentinelBoot.isUserUnlocked(this) || SentinelRing.uiFailed(this))

    private fun bringUpBackupScreenIfNeeded() {
        val plan = (ringing[owner] ?: ringing.values.firstOrNull())?.plan ?: return
        if (!needsBackup(plan)) return
        showForeground()
        if (backupLaunched) return
        backupLaunched = true
        // Allowed with the overlay permission; the full-screen notification is the
        // way in otherwise.
        try {
            startActivity(
                Intent(this, SentinelBackupActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        } catch (e: Throwable) {
            Log.w(TAG, "starting the backup screen failed: ${e.message}")
        }
    }

    /** Rust ends the alarm itself: this cannot, and the app may be dead. */
    private fun endRingsPastTheirLimit() {
        val now = System.currentTimeMillis()
        val lastActivity = SentinelRing.lastActivityMs(this)
        for (ring in ringing.values.toList()) {
            val limitSecs = ring.plan.optLong("ring_limit_secs", 0)
            if (limitSecs > 0 && now - maxOf(ring.sinceMs, lastActivity) >= limitSecs * 1000) {
                Log.i(TAG, "${ring.id} rang for its limit of ${limitSecs}s")
                SentinelRing.forget(this, ring.id)
                drop(ring.id)
                SentinelActiveJobs.markLimitReached(this, ring.id)
            }
        }
    }

    // ---- Sound and vibration ------------------------------------------------

    private fun startOutputs(ring: SentinelRing.Ringing, fade: Boolean) {
        cancelSoundLater()
        owner = ring.id
        val plan = ring.plan
        SentinelRing.holdVolume(this, plan.optInt("min_volume_percent", 0))
        val fadeInMs = if (fade) plan.optLong("fade_in_ms", 0) else 0L
        val sounds = soundChain(plan)
        Log.i(TAG, "ringing ${ring.id}: ${sounds.size} sound(s), fade ${fadeInMs}ms")
        val pattern = vibrationPattern(plan)
        val vibrating = plan.optBoolean("vibrate", !plan.has("sound_uri"))
        if (sounds.isNotEmpty()) {
            val playSound = Runnable {
                soundLater = null
                SentinelFiringAudioPrimitives.play(
                    sounds,
                    plan.optString("audio_usage", "alarm"),
                    plan.optString("audio_content_type", "sonification"),
                    plan.optBoolean("looping", true),
                    fadeInMs,
                ) {
                    // No sound at all could be played: never ring silently.
                    SentinelRing.logTiming(this, "no_sound", ring.id)
                    if (!vibrating) SentinelFiringVibrationPrimitives.startVibration(pattern)
                }
                SentinelRing.logTiming(this, "sound", ring.id)
            }
            // Only at the very start: a ring that comes back after anything else
            // has waited long enough.
            val delayMs = if (fade && vibrating) plan.optLong("sound_delay_ms", 0) else 0L
            if (delayMs > 0) {
                soundLater = playSound
                handler.postDelayed(playSound, delayMs)
            } else {
                playSound.run()
            }
        }
        if (vibrating) SentinelFiringVibrationPrimitives.startVibration(pattern)
    }

    private fun cancelSoundLater() {
        soundLater?.let { handler.removeCallbacks(it) }
        soundLater = null
    }

    private fun stopOutputs() {
        cancelSoundLater()
        SentinelFiringAudioPrimitives.stop()
        SentinelFiringVibrationPrimitives.stopVibration()
    }

    /** The sounds to try, best first. Empty for an alarm that is silent on purpose. */
    private fun soundChain(plan: JSONObject): List<String> {
        if (!plan.has("sound_uri")) return listOf(DEFAULT_SOUND)
        val chosen = plan.optString("sound_uri", "")
        if (chosen.isEmpty() || chosen == SILENT_SOUND) return emptyList()
        val chain = arrayListOf(chosen)
        val fallbacks = plan.optJSONArray("fallback_sound_uris") ?: JSONArray()
        for (i in 0 until fallbacks.length()) chain.add(fallbacks.optString(i))
        return chain.filter { it.isNotEmpty() && it != SILENT_SOUND }.distinct()
    }

    private fun vibrationPattern(plan: JSONObject): LongArray {
        val raw = plan.optJSONArray("vibration_pattern")
        if (raw == null || raw.length() == 0) return DEFAULT_VIBRATION
        return LongArray(raw.length()) { raw.optLong(it) }
    }

    // ---- Notification -------------------------------------------------------

    /** Show the foreground notification, posting again only if it changed. */
    private fun showForeground() {
        val plan = (ringing[owner] ?: ringing.values.firstOrNull())?.plan ?: JSONObject()
        val backup = needsBackup(plan)
        val fullScreen = plan.optBoolean("firing_full_screen", true) && (!uiVisible || backup)
        if (postedOwner == owner && postedOwner != null &&
            postedFullScreen == fullScreen && postedBackup == backup
        ) {
            return
        }
        try {
            startForeground(NOTIFICATION_ID, buildNotification(plan, fullScreen, backup))
            postedOwner = owner
            postedFullScreen = fullScreen
            postedBackup = backup
            Log.i(TAG, "foreground, owner=$owner fullScreen=$fullScreen backup=$backup")
        } catch (e: Throwable) {
            // Not allowed from here. The sound plays on for now and the watchdog,
            // which is allowed to, retries.
            Log.e(TAG, "startForeground failed: ${e.message}", e)
        }
    }

    private fun ensureChannel(plan: JSONObject, channelId: String) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = getSystemService(NotificationManager::class.java) ?: return
        val name = plan.optString("channel_name", "").ifEmpty { "Alarms" }
        val importance = plan.optInt("importance", NotificationManager.IMPORTANCE_MAX)
        // Creating an existing channel only updates its name, for a language change.
        val channel = NotificationChannel(channelId, name, importance).apply {
            setBypassDnd(plan.optBoolean("bypass_dnd", true))
            lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            setSound(null, null)
        }
        nm.createNotificationChannel(channel)
    }

    private fun buildNotification(plan: JSONObject, fullScreen: Boolean, backup: Boolean): Notification {
        val channelId = plan.optString("channel_id", "").ifEmpty { FALLBACK_CHANNEL }
        ensureChannel(plan, channelId)
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, channelId)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this).setPriority(Notification.PRIORITY_MAX)
        }
        builder.setContentTitle(plan.optString("title", "").ifEmpty { "Alarm" })
            .setContentText(plan.optString("body", ""))
            .setSmallIcon(android.R.drawable.ic_lock_idle_alarm)
            .setOngoing(true)
            .setOnlyAlertOnce(!fullScreen)
            .setCategory(Notification.CATEGORY_ALARM)
            .setVisibility(Notification.VISIBILITY_PUBLIC)

        val activity = if (backup) {
            SentinelBackupActivity::class.java.name
        } else {
            plan.optString("activity_fqcn", "")
        }
        if (activity.isNotEmpty()) {
            val tap = Intent().apply {
                component = ComponentName(packageName, activity)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or
                    Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or
                    Intent.FLAG_ACTIVITY_SINGLE_TOP
            }
            val pi = PendingIntent.getActivity(
                this,
                0,
                tap,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            builder.setContentIntent(pi)
            if (fullScreen) builder.setFullScreenIntent(pi, true)
        }
        return builder.build()
    }
}
